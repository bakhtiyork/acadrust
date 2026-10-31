// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp's TextProcessor.cs by Albert Domenech.
// Copyright (c) 2021 Albert Domenech
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! ACadSharp-compatible plain-text projection for MTEXT values.
//!
//! This intentionally follows `ACadSharp.Text.TextProcessor.Parse`, rather
//! than the native MTEXT parser. The two parsers have different compatibility
//! behavior for unknown controls, groups, and semicolon-terminated controls.

/// Return the plain-text form produced by ACadSharp's MTEXT processor.
///
/// ACadSharp removes unescaped group braces, resolves escaped braces and
/// backslashes, emits LF for `\P` and `\n`, and skips the formatting controls
/// it recognizes through their first semicolon. Unknown controls lose only
/// their leading backslash. A malformed recognized control without a
/// semicolon consumes the remainder of the input: ACadSharp's implementation
/// resets its index to zero in that case and can loop forever, while this
/// compatibility boundary must terminate for untrusted input. The C# font
/// helper is a separate malformed case: `\f`/`\F` at a nonzero offset throws
/// `ArgumentOutOfRangeException`, represented here by the canonical sentinel.
pub(crate) fn plain_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return text.to_owned();
    }

    let mut result = String::with_capacity(text.len());
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];
        let previous = index.checked_sub(1).and_then(|i| chars.get(i).copied());
        let next = chars.get(index + 1).copied();

        if current == '\\' {
            match next {
                Some('}' | '{' | '\\') => {
                    result.push(next.expect("matched escaped character"));
                    index += 2;
                }
                Some('f' | 'F') => match skip_control(&chars, index) {
                    Some(end) => index = end,
                    None if index == 0 => index = chars.len(),
                    None => return "!throws:ArgumentOutOfRangeException".to_owned(),
                },
                Some('A' | 'c' | 'C' | 'h' | 'H' | 'p') => {
                    index = skip_control(&chars, index).unwrap_or(chars.len());
                }
                Some('P' | 'n') => {
                    // ACadSharp uses Environment.NewLine; the compatibility
                    // dump is platform-independent and always emits LF.
                    result.push('\n');
                    index += 2;
                }
                Some(_) => {
                    // Match TextProcessor.Parse's default branch: consume the
                    // slash, then let the control letter be handled normally.
                    index += 1;
                }
                None => {
                    result.push('\\');
                    index += 1;
                }
            }
        } else if current == '{' && previous != Some('\\') {
            index += 1;
        } else if current == '}' && previous != Some('\\') {
            index += 1;
        } else {
            result.push(current);
            index += 1;
        }
    }

    result
}

/// Find the end of a recognized ACadSharp formatting control at its first `;`.
///
/// `None` deliberately maps to the end of the input. The C# helper uses
/// `IndexOf(';') + 1`, which becomes zero when no semicolon exists and causes
/// an infinite loop; making progress here is the required malformed-input
/// boundary behavior.
fn skip_control(chars: &[char], start: usize) -> Option<usize> {
    chars[start..]
        .iter()
        .position(|&character| character == ';')
        .map(|offset| start + offset + 1)
}

#[cfg(test)]
mod tests {
    use super::plain_text;

    #[test]
    fn removes_formatting_groups_and_semicolon_controls() {
        let input = r"- Font: {\fCalibri|b0|i0;Calibri\Fcdm|c0; CDM \fConsolas|b0|i0;Consolas\P}";
        assert_eq!(plain_text(input), "- Font: Calibri CDM Consolas\n");

        let input = r"{\C3;green} {\H2x;high} {\pxqc;center}";
        assert_eq!(plain_text(input), "green high center");
    }

    #[test]
    fn resolves_escapes_and_line_controls() {
        assert_eq!(plain_text(r"\{ \} \\ \P \n"), "{ } \\ \n \n");
        assert_eq!(plain_text(r"a\Pb\nc"), "a\nb\nc");
        assert_eq!(plain_text(r"trailing\"), "trailing\\");
    }

    #[test]
    fn unknown_controls_only_drop_the_backslash() {
        assert_eq!(plain_text(r"\Z \S1/2; \W1; \;"), "Z S1/2; W1; ;");
    }

    #[test]
    fn semicolon_is_literal_without_a_known_control() {
        assert_eq!(plain_text("a;b;c"), "a;b;c");
        assert_eq!(plain_text(r"left\z;right"), "leftz;right");
    }

    #[test]
    fn preserves_unicode_scalars() {
        assert_eq!(plain_text("é 漢字 🙂 \\P Ω"), "é 漢字 🙂 \n Ω");
    }

    #[test]
    fn unterminated_known_controls_consume_to_end() {
        assert_eq!(plain_text(r"prefix\Aunterminated"), "prefix");
        assert_eq!(plain_text(r"prefix\C1;ok\Hunterminated"), "prefixok");
    }

    #[test]
    fn unterminated_font_controls_match_the_csharp_exception_boundary() {
        assert_eq!(
            plain_text(r"prefix\fArial|b0"),
            "!throws:ArgumentOutOfRangeException"
        );
        assert_eq!(
            plain_text(r"prefix\FArial|b0"),
            "!throws:ArgumentOutOfRangeException"
        );
        // At offset zero C# loops after its zero-length substring; consume to
        // end instead so malformed untrusted input remains finite.
        assert_eq!(plain_text(r"\fArial|b0"), "");
        assert_eq!(plain_text(r"\FArial|b0"), "");
    }
}
