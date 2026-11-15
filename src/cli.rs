use crate::{
    albums,
    copier::{self, DestLayout},
    device::{self, DeviceStatus, MountGuard},
    scanner::{self, Bucket, MediaItem, ScanIndex},
};
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[derive(Parser, Debug)]
#[command(
    name = "copyzer",
    version,
    about = "Fast, keyboard-first iPhone media copier for Arch Linux & Omarchy"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub mount_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pub dest: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Scan {
        #[arg(long)]
        json: bool,
    },
    Copy {
        #[arg(long)]
        year: Vec<i32>,
        #[arg(long)]
        month: Vec<String>,
        #[arg(long)]
        album: Vec<String>,
        #[arg(long)]
        dcim: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        flat: bool,
        #[arg(long)]
        json: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let dest = cli.dest.unwrap_or_else(default_dest);

    match cli.command {
        None => crate::tui::run(cli.mount_dir, Some(dest)),
        Some(Commands::Doctor { json }) => run_doctor(json),
        Some(Commands::Scan { json }) => run_scan(cli.mount_dir, json),
        Some(Commands::Copy {
            year,
            month,
            album,
            dcim,
            all,
            flat,
            json,
        }) => run_copy(
            cli.mount_dir,
            dest,
            CopyArgs {
                year,
                month,
                album,
                dcim,
                all,
                flat,
                json,
            },
        ),
    }
}

struct CopyArgs {
    year: Vec<i32>,
    month: Vec<String>,
    album: Vec<String>,
    dcim: Vec<String>,
    all: bool,
    flat: bool,
    json: bool,
}

fn default_dest() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Pictures")
        })
        .join("Copyzer")
}

fn resolve_mount(mount_dir: Option<PathBuf>) -> Result<MountGuard> {
    if let Some(path) = mount_dir {
        return Ok(MountGuard::from_existing(path));
    }

    match device::detect_device() {
        DeviceStatus::Connected(info) => device::mount_iphone(&info.udid),
        DeviceStatus::Untrusted { message, .. } => bail!("{message}"),
        DeviceStatus::NoDevice => {
            bail!("No iPhone detected. Connect via USB, unlock it, and tap Trust.")
        }
        DeviceStatus::MissingTools(tools) => {
            bail!(
                "Missing required tools: {}. Install libimobiledevice and ifuse.",
                tools.join(", ")
            )
        }
    }
}

fn run_doctor(json: bool) -> Result<()> {
    let report = device::doctor_report();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Tools:");
    for (tool, available) in &report.tools {
        let marker = if *available { "ok" } else { "missing" };
        println!("  [{marker}] {tool}");
    }

    println!("Device:");
    match &report.status {
        DeviceStatus::Connected(info) => {
            println!(
                "  connected: {} ({}, iOS {}, {})",
                info.name, info.product_type, info.ios_version, info.udid
            );
        }
        DeviceStatus::Untrusted { udid, message } => {
            println!("  untrusted ({udid}): {message}");
        }
        DeviceStatus::NoDevice => {
            println!("  no device detected");
            if report.usb_a_fastcharge_warning {
                println!(
                    "  hint (USB-A cable): 'apple_mfi_fastcharge' kernel module is loaded and can brown out USB-A ports."
                );
                println!("  fix: sudo modprobe -r apple_mfi_fastcharge");
            }
        }
        DeviceStatus::MissingTools(tools) => {
            println!("  cannot check device; missing tools: {}", tools.join(", "));
        }
    }

    Ok(())
}

fn run_scan(mount_dir: Option<PathBuf>, json: bool) -> Result<()> {
    let guard = resolve_mount(mount_dir)?;
    let index = scanner::scan_dcim(guard.path())?;
    let album_res = albums::load_albums_with_udid(guard.path(), &index, guard.udid());

    if json {
        let payload = serde_json::json!({
            "index": index,
            "albums": album_res.albums,
            "warning": album_res.warning,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!(
        "Scanned {} items ({})",
        index.items.len(),
        scanner::format_bytes(index.total_bytes)
    );
    print_buckets("Years", &index.years);
    print_buckets("Months", &index.months);
    print_buckets("DCIM folders", &index.dcim_folders);
    if let Some(warning) = &album_res.warning {
        println!("Albums: unavailable ({warning})");
    } else {
        print_buckets("Albums", &album_res.albums);
    }

    Ok(())
}

fn print_buckets(heading: &str, buckets: &[Bucket]) {
    if buckets.is_empty() {
        println!("{heading}: none");
        return;
    }

    println!("{heading} ({}):", buckets.len());
    for b in buckets {
        println!(
            "  {} — {} items ({})",
            b.label,
            b.item_indices.len(),
            scanner::format_bytes(b.total_bytes)
        );
    }
}

fn run_copy(mount_dir: Option<PathBuf>, dest: PathBuf, args: CopyArgs) -> Result<()> {
    if !args.all
        && args.year.is_empty()
        && args.month.is_empty()
        && args.album.is_empty()
        && args.dcim.is_empty()
    {
        bail!(
            "No media selection specified. Pass --year, --month, --album, --dcim, or --all, or run `copyzer` with no subcommand to open the TUI."
        );
    }

    let guard = resolve_mount(mount_dir)?;
    let index = scanner::scan_dcim(guard.path())?;
    let album_buckets = if args.album.is_empty() {
        Vec::new()
    } else {
        let res = albums::load_albums_with_udid(guard.path(), &index, guard.udid());
        if let Some(warning) = res.warning {
            eprintln!("Warning: {warning}");
        }
        res.albums
    };

    let indices = select_item_indices(&index, &album_buckets, &args);
    let selected: Vec<&MediaItem> = indices.iter().map(|&i| &index.items[i]).collect();
    let layout = resolve_layout(&args);
    let cancel = AtomicBool::new(false);

    let summary = copier::copy_items(&selected, &dest, &layout, &cancel, |_| {})?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    println!(
        "Copy complete to {}: {} copied ({}), {} skipped, {} failed",
        dest.display(),
        summary.copied_files,
        scanner::format_bytes(summary.copied_bytes),
        summary.skipped_files,
        summary.failed_files.len(),
    );
    for (file, err) in &summary.failed_files {
        eprintln!("  failed: {file}: {err}");
    }

    Ok(())
}

fn select_item_indices(index: &ScanIndex, albums: &[Bucket], args: &CopyArgs) -> Vec<usize> {
    if args.all {
        return (0..index.items.len()).collect();
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for &y in &args.year {
        let key = format!("{y:04}");
        extend_from_buckets(&index.years, &key, &mut out, &mut seen);
    }
    for m in &args.month {
        extend_from_buckets(&index.months, m, &mut out, &mut seen);
    }
    for a in &args.album {
        extend_from_buckets(albums, a, &mut out, &mut seen);
    }
    for d in &args.dcim {
        extend_from_buckets(&index.dcim_folders, d, &mut out, &mut seen);
    }

    out
}

fn extend_from_buckets(
    buckets: &[Bucket],
    key: &str,
    out: &mut Vec<usize>,
    seen: &mut HashSet<usize>,
) {
    for bucket in buckets.iter().filter(|b| b.key == key) {
        for &idx in &bucket.item_indices {
            if seen.insert(idx) {
                out.push(idx);
            }
        }
    }
}

fn resolve_layout(args: &CopyArgs) -> DestLayout {
    let only_single_album = args.flat
        && !args.all
        && args.year.is_empty()
        && args.month.is_empty()
        && args.dcim.is_empty()
        && args.album.len() == 1;
    if only_single_album {
        return DestLayout::FlatBucket(args.album[0].clone());
    }

    let only_single_dcim = args.flat
        && !args.all
        && args.year.is_empty()
        && args.month.is_empty()
        && args.album.is_empty()
        && args.dcim.len() == 1;
    if only_single_dcim {
        return DestLayout::FlatBucket(args.dcim[0].clone());
    }

    DestLayout::YearMonth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_doctor_json() {
        let cli = Cli::try_parse_from(["copyzer", "doctor", "--json"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Doctor { json: true })));
    }

    #[test]
    fn parse_scan_with_mount_dir() {
        let cli = Cli::try_parse_from(["copyzer", "scan", "--mount-dir", "/tmp/mnt"]).unwrap();
        assert_eq!(cli.mount_dir, Some(PathBuf::from("/tmp/mnt")));
        assert!(matches!(cli.command, Some(Commands::Scan { json: false })));
    }

    #[test]
    fn parse_copy_year_and_month() {
        let cli =
            Cli::try_parse_from(["copyzer", "copy", "--year", "2025", "--month", "2025-11"])
                .unwrap();
        match cli.command {
            Some(Commands::Copy {
                year,
                month,
                album,
                dcim,
                all,
                flat,
                json,
            }) => {
                assert_eq!(year, vec![2025]);
                assert_eq!(month, vec!["2025-11"]);
                assert!(album.is_empty());
                assert!(dcim.is_empty());
                assert!(!all);
                assert!(!flat);
                assert!(!json);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
