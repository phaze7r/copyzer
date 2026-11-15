/// Pashto / Arabic Right-to-Left (RTL) contextual letter shaping and BiDi visual
/// reordering for terminal UI cells (`ratatui`), while leaving on-disk filenames untouched.

#[derive(Clone, Copy, PartialEq, Eq)]
enum JoinType {
    NonJoining,
    RightJoining, // Joins only to preceding dual-joining letter (e.g. د, ډ, ر, ړ, ږ, و, ۍ, ا)
    DualJoining,  // Joins to both preceding and following letters (e.g. ب, پ, ت, ټ, ځ, څ, س, ښ, ګ, ڼ, ی, ې)
    Transparent,  // Combining marks / harakat
}

#[derive(Clone, Copy)]
struct ShapingForms {
    isolated: char,
    final_form: char,
    initial: char,
    medial: char,
    join: JoinType,
}

/// Returns true if the string contains any Arabic, Pashto, Persian, or Urdu characters.
pub fn contains_rtl(s: &str) -> bool {
    s.chars().any(is_rtl_char)
}

fn is_rtl_char(c: char) -> bool {
    matches!(
        c as u32,
        0x0590..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF
    )
}

fn is_transparent_mark(c: char) -> bool {
    matches!(c as u32, 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06ED)
}

/// Converts logical UTF-8 text containing Pashto/Arabic into visual order with contextual
/// Arabic/Persian/Pashto presentation forms so terminal grids render Pashto naturally.
pub fn shape_for_terminal(input: &str) -> String {
    if !contains_rtl(input) {
        return input.to_string();
    }

    let chars: Vec<char> = input
        .chars()
        .filter(|&c| c != '\u{200C}' && c != '\u{200D}' && c != '\u{200E}' && c != '\u{200F}')
        .collect();

    // 1. Contextual Arabic/Pashto glyph shaping in logical order
    let shaped = shape_contextual(&chars);

    // 2. BiDi visual reordering for LTR terminal cell grid
    reorder_bidi_runs(&shaped)
}

fn join_type_of(c: char) -> JoinType {
    if is_transparent_mark(c) {
        return JoinType::Transparent;
    }
    if let Some(forms) = lookup_forms(c) {
        return forms.join;
    }
    // Pashto-specific letters without dedicated FB50/FE70 presentation form codepoints
    // still participate in neighbor joining so adjacent letters take Initial/Medial/Final forms.
    match c {
        // Pashto right-joining letters (Dal/Reh/Waw/Ye-tail families)
        'ډ' | 'ړ' | 'ږ' | 'ۍ' | 'ۀ' | 'ؤ' | 'إ' | 'أ' | 'آ' | 'ا' | 'د' | 'ذ' | 'ر'
        | 'ز' | 'ژ' | 'و' | 'ة' => JoinType::RightJoining,
        // Pashto dual-joining letters (Teh/Hah/Seen/Gaf/Noon/Ye families)
        'ټ' | 'ځ' | 'څ' | 'ښ' | 'ګ' | 'ڼ' | 'ې' => JoinType::DualJoining,
        _ => JoinType::NonJoining,
    }
}

fn prev_joins_left(chars: &[char], idx: usize) -> bool {
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let jt = join_type_of(chars[i]);
        if jt == JoinType::Transparent {
            continue;
        }
        return jt == JoinType::DualJoining;
    }
    false
}

fn next_joins_right(chars: &[char], idx: usize) -> bool {
    let mut i = idx + 1;
    while i < chars.len() {
        let jt = join_type_of(chars[i]);
        if jt == JoinType::Transparent {
            i += 1;
            continue;
        }
        return matches!(jt, JoinType::DualJoining | JoinType::RightJoining);
    }
    false
}

fn shape_contextual(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Handle Lam-Alef ligatures (لا, لأ, لإ, لآ)
        if c == 'ل' && i + 1 < chars.len() {
            if let Some((isolated_lig, final_lig)) = lam_alef_ligature(chars[i + 1]) {
                let joins_prev = prev_joins_left(chars, i);
                out.push(if joins_prev { final_lig } else { isolated_lig });
                i += 2;
                continue;
            }
        }

        if let Some(forms) = lookup_forms(c) {
            let joins_prev = prev_joins_left(chars, i)
                && matches!(forms.join, JoinType::DualJoining | JoinType::RightJoining);
            let joins_next =
                next_joins_right(chars, i) && matches!(forms.join, JoinType::DualJoining);

            let glyph = match (joins_prev, joins_next) {
                (true, true) => forms.medial,
                (true, false) => forms.final_form,
                (false, true) => forms.initial,
                (false, false) => forms.isolated,
            };
            out.push(glyph);
        } else {
            out.push(c);
        }

        i += 1;
    }

    out
}

fn lam_alef_ligature(next: char) -> Option<(char, char)> {
    match next {
        'آ' => Some(('\u{FEF5}', '\u{FEF6}')),
        'أ' => Some(('\u{FEF7}', '\u{FEF8}')),
        'إ' => Some(('\u{FEF9}', '\u{FEFA}')),
        'ا' => Some(('\u{FEFB}', '\u{FEFC}')),
        _ => None,
    }
}

fn lookup_forms(c: char) -> Option<ShapingForms> {
    use JoinType::{DualJoining as D, NonJoining as N, RightJoining as R};
    let (iso, fin, ini, med, join) = match c {
        'ء' => ('\u{FE80}', '\u{FE80}', '\u{FE80}', '\u{FE80}', N),
        'آ' => ('\u{FE81}', '\u{FE82}', '\u{FE81}', '\u{FE82}', R),
        'أ' => ('\u{FE83}', '\u{FE84}', '\u{FE83}', '\u{FE84}', R),
        'ؤ' => ('\u{FE85}', '\u{FE86}', '\u{FE85}', '\u{FE86}', R),
        'إ' => ('\u{FE87}', '\u{FE88}', '\u{FE87}', '\u{FE88}', R),
        'ئ' => ('\u{FE89}', '\u{FE8A}', '\u{FE8B}', '\u{FE8C}', D),
        'ا' => ('\u{FE8D}', '\u{FE8E}', '\u{FE8D}', '\u{FE8E}', R),
        'ب' => ('\u{FE8F}', '\u{FE90}', '\u{FE91}', '\u{FE92}', D),
        'پ' => ('\u{FB56}', '\u{FB57}', '\u{FB58}', '\u{FB59}', D),
        'ة' => ('\u{FE93}', '\u{FE94}', '\u{FE93}', '\u{FE94}', R),
        'ت' => ('\u{FE95}', '\u{FE96}', '\u{FE97}', '\u{FE98}', D),
        'ث' => ('\u{FE99}', '\u{FE9A}', '\u{FE9B}', '\u{FE9C}', D),
        'ج' => ('\u{FE9D}', '\u{FE9E}', '\u{FE9F}', '\u{FEA0}', D),
        'چ' => ('\u{FB7A}', '\u{FB7B}', '\u{FB7C}', '\u{FB7D}', D),
        'ح' => ('\u{FEA1}', '\u{FEA2}', '\u{FEA3}', '\u{FEA4}', D),
        'خ' => ('\u{FEA5}', '\u{FEA6}', '\u{FEA7}', '\u{FEA8}', D),
        'د' => ('\u{FEA9}', '\u{FEAA}', '\u{FEA9}', '\u{FEAA}', R),
        'ذ' => ('\u{FEAB}', '\u{FEAC}', '\u{FEAB}', '\u{FEAC}', R),
        'ر' => ('\u{FEAD}', '\u{FEAE}', '\u{FEAD}', '\u{FEAE}', R),
        'ز' => ('\u{FEAF}', '\u{FEB0}', '\u{FEAF}', '\u{FEB0}', R),
        'ژ' => ('\u{FB8A}', '\u{FB8B}', '\u{FB8A}', '\u{FB8B}', R),
        'س' => ('\u{FEB1}', '\u{FEB2}', '\u{FEB3}', '\u{FEB4}', D),
        'ش' => ('\u{FEB5}', '\u{FEB6}', '\u{FEB7}', '\u{FEB8}', D),
        'ص' => ('\u{FEB9}', '\u{FEBA}', '\u{FEBB}', '\u{FEBC}', D),
        'ض' => ('\u{FEBD}', '\u{FEBE}', '\u{FEBF}', '\u{FEC0}', D),
        'ط' => ('\u{FEC1}', '\u{FEC2}', '\u{FEC3}', '\u{FEC4}', D),
        'ظ' => ('\u{FEC5}', '\u{FEC6}', '\u{FEC7}', '\u{FEC8}', D),
        'ع' => ('\u{FEC9}', '\u{FECA}', '\u{FECB}', '\u{FECC}', D),
        'غ' => ('\u{FECD}', '\u{FECE}', '\u{FECF}', '\u{FED0}', D),
        'ف' => ('\u{FED1}', '\u{FED2}', '\u{FED3}', '\u{FED4}', D),
        'ق' => ('\u{FED5}', '\u{FED6}', '\u{FED7}', '\u{FED8}', D),
        'ك' => ('\u{FED9}', '\u{FEDA}', '\u{FEDB}', '\u{FEDC}', D),
        'ک' => ('\u{FB8E}', '\u{FB8F}', '\u{FB90}', '\u{FB91}', D),
        'گ' => ('\u{FB92}', '\u{FB93}', '\u{FB94}', '\u{FB95}', D),
        'ل' => ('\u{FEDD}', '\u{FEDE}', '\u{FEDF}', '\u{FEE0}', D),
        'م' => ('\u{FEE1}', '\u{FEE2}', '\u{FEE3}', '\u{FEE4}', D),
        'ن' => ('\u{FEE5}', '\u{FEE6}', '\u{FEE7}', '\u{FEE8}', D),
        'ڼ' => ('\u{FB9E}', '\u{FB9F}', '\u{FEE7}', '\u{FEE8}', D),
        'ه' => ('\u{FEE9}', '\u{FEEA}', '\u{FEEB}', '\u{FEEC}', D),
        'ۀ' => ('\u{FBA4}', '\u{FBA5}', '\u{FBA4}', '\u{FBA5}', R),
        'و' => ('\u{FEED}', '\u{FEEE}', '\u{FEED}', '\u{FEEE}', R),
        'ى' => ('\u{FEEF}', '\u{FEF0}', '\u{FEEF}', '\u{FEF0}', R),
        'ي' => ('\u{FEF1}', '\u{FEF2}', '\u{FEF3}', '\u{FEF4}', D),
        'ی' => ('\u{FBFC}', '\u{FBFD}', '\u{FBFE}', '\u{FBFF}', D),
        'ې' => ('\u{FBE4}', '\u{FBE5}', '\u{FBE6}', '\u{FBE7}', D),
        'ے' => ('\u{FBAE}', '\u{FBAF}', '\u{FBAE}', '\u{FBAF}', R),
        _ => return None,
    };

    Some(ShapingForms {
        isolated: iso,
        final_form: fin,
        initial: ini,
        medial: med,
        join,
    })
}

/// Reorders contiguous RTL segments (including spaces between RTL words) into visual
/// LTR order for terminal rendering while keeping LTR words/numbers intact.
fn reorder_bidi_runs(chars: &[char]) -> String {
    let mut result = String::with_capacity(chars.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        if is_rtl_char(chars[i]) {
            let start = i;
            let mut end = i + 1;
            let mut probe = end;
            while probe < chars.len() {
                if is_rtl_char(chars[probe]) {
                    end = probe + 1;
                    probe += 1;
                } else if chars[probe].is_whitespace() {
                    probe += 1;
                } else {
                    break;
                }
            }
            for &ch in chars[start..end].iter().rev() {
                result.push(ch);
            }
            i = end;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_pure_ascii_and_english_untouched() {
        assert!(!contains_rtl("Project Indigo"));
        assert_eq!(shape_for_terminal("Project Indigo"), "Project Indigo");
    }

    #[test]
    fn shapes_and_reorders_pashto_words_and_mixed_labels() {
        let pashto = "پښتو";
        assert!(contains_rtl(pashto));
        let shaped = shape_for_terminal(pashto);
        // Should reverse visual order so rightmost initial 'پ' is at the end of the LTR cell run
        assert_ne!(shaped, pashto);
        assert_eq!(shaped.chars().count(), 4);

        // Mixed Pashto device name + English model badge keeps LTR model intact
        let mixed = "پښتو · iPhone 16 Pro";
        let rendered = shape_for_terminal(mixed);
        assert!(rendered.ends_with(" · iPhone 16 Pro"));
    }
}
