//! 터미널 표시 폭 — cc-usage `internal/render/width.go` 의 이식. ANSI 이스케이프는 0칸, 한글·이모지는 2칸.
//! 표준 라이브러리에 wcwidth 가 없어 statusline 에 실제로 나오는 구간만 담은 근사다(새 의존성을 들이지 않는다).

const ZWJ: char = '\u{200D}';
const VS16: char = '\u{FE0F}';

/// 문자열이 터미널에서 차지하는 칸 수.
pub fn display_width(s: &str) -> usize {
    let mut w = 0;
    let mut joined = false;
    // 직전 글자가 더한 폭 — VS16 이 1칸 글자를 2칸으로 올린다.
    let mut last = 0;
    let mut chars = s.chars().peekable();
    while let Some(r) = chars.next() {
        if r == '\x1b' {
            // CSI(`ESC [ … 끝 글자`)는 통째로 건너뛰고, 그 밖의 ESC 는 ESC 만 건너뛴다.
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        if r == VS16 {
            if last == 1 {
                w += 1;
                last = 2;
            }
        } else if r == ZWJ {
            joined = true;
        } else if joined {
            joined = false; // 앞 글자에 합쳐졌다 — 폭을 더하지 않는다
        } else {
            last = rune_width(r);
            w += last;
        }
    }
    w
}

fn rune_width(r: char) -> usize {
    let r = r as u32;
    if r < 0x1100 {
        return 1;
    }
    if (0xFE00..=0xFE0F).contains(&r) || (0x200B..=0x200F).contains(&r) {
        return 0; // variation selector · zero-width 계열
    }
    // 이모지 표현이 기본인 BMP 기호.
    const WIDE_SYMBOLS: [u32; 56] = [
        0x231A, 0x231B, 0x23E9, 0x23EA, 0x23EB, 0x23EC, 0x23F0, 0x23F3, 0x25FD, 0x25FE, 0x2614,
        0x2615, 0x2648, 0x2649, 0x264A, 0x264B, 0x264C, 0x264D, 0x264E, 0x264F, 0x2650, 0x2651,
        0x2652, 0x2653, 0x267F, 0x2693, 0x26A1, 0x26AA, 0x26AB, 0x26BD, 0x26BE, 0x26C4, 0x26C5,
        0x26CE, 0x26D4, 0x26EA, 0x26F2, 0x26F3, 0x26F5, 0x26FA, 0x26FD, 0x2705, 0x270A, 0x270B,
        0x2728, 0x274C, 0x274E, 0x2753, 0x2754, 0x2755, 0x2757, 0x2795, 0x2796, 0x2797, 0x27B0,
        0x27BF,
    ];
    if WIDE_SYMBOLS.contains(&r) {
        return 2;
    }
    let wide = r <= 0x115F // 한글 자모
        || (0x2E80..=0x303E).contains(&r) // CJK 부수 · 기호
        || (0x3041..=0x33FF).contains(&r)
        || (0x3400..=0x4DBF).contains(&r)
        || (0x4E00..=0x9FFF).contains(&r)
        || (0xA000..=0xA4CF).contains(&r)
        || (0xAC00..=0xD7A3).contains(&r) // 한글 음절
        || (0xF900..=0xFAFF).contains(&r)
        || (0xFE30..=0xFE6F).contains(&r)
        || (0xFF00..=0xFF60).contains(&r) // 전각
        || (0xFFE0..=0xFFE6).contains(&r)
        || (0x1F300..=0x1F64F).contains(&r) // 기호·그림 · 감정 (🏢 포함)
        || (0x1F680..=0x1F6C5).contains(&r) // 🚀 … 🛅
        || r == 0x1F6CC // 🛌
        || (0x1F6D0..=0x1F6D2).contains(&r)
        || (0x1F6D5..=0x1F6D7).contains(&r)
        || (0x1F6DC..=0x1F6DF).contains(&r)
        || (0x1F6EB..=0x1F6EC).contains(&r) // 🛫 🛬
        || (0x1F6F4..=0x1F6FC).contains(&r)
        || (0x1F7E0..=0x1F7EB).contains(&r) // 색 원·사각 (🟠 🟦)
        || (0x1F900..=0x1F9FF).contains(&r) // 보충 기호 (🧠 🩵)
        || (0x1FA70..=0x1FAFF).contains(&r); // 확장 A (🪄 🫠)
    if wide {
        2
    } else {
        1
    }
}
