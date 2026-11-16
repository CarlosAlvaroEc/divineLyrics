use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::{LyricLine, LyricsPayload};

/// Expresión regular para etiquetas de tiempo LRC: `[mm:ss.xx]` o `[mm:ss.xxx]`.
static LRC_TAG_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[(\d{2,}):(\d{2})\.(\d{2,3})\]").expect("Invalid LRC timestamp regex")
});

/// Parsea una cadena en formato LRC sincronizado (`[mm:ss.xx]` o `[mm:ss.xxx]`)
/// y devuelve las líneas ordenadas ascendentemente por `time_ms`.
pub fn parse_synced_lrc(raw_lrc: &str) -> Vec<LyricLine> {
    let mut parsed_lines = Vec::new();

    for raw_line in raw_lrc.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut timestamps_ms = Vec::new();
        let mut last_match_end = 0;

        for caps in LRC_TAG_REGEX.captures_iter(trimmed) {
            if let Some(full_match) = caps.get(0) {
                last_match_end = full_match.end();
            }

            let minutes: u64 = match caps.get(1).and_then(|m| m.as_str().parse().ok()) {
                Some(v) => v,
                None => continue,
            };
            let seconds: u64 = match caps.get(2).and_then(|m| m.as_str().parse().ok()) {
                Some(v) => v,
                None => continue,
            };
            let frac_str = match caps.get(3).map(|m| m.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let frac_val: u64 = match frac_str.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };

            let millis = if frac_str.len() == 2 {
                frac_val * 10
            } else {
                frac_val
            };

            let total_ms = (minutes * 60 + seconds) * 1000 + millis;
            timestamps_ms.push(total_ms);
        }

        if timestamps_ms.is_empty() {
            continue;
        }

        let lyric_text = trimmed[last_match_end..].trim().to_string();
        for time_ms in timestamps_ms {
            parsed_lines.push(LyricLine {
                time_ms,
                text: lyric_text.clone(),
            });
        }
    }

    parsed_lines.sort_by_key(|line| line.time_ms);
    parsed_lines
}

/// Parsea letras planas sin sincronizar asignando `time_ms: 0` a cada línea.
pub fn parse_plain_lyrics(raw_plain: &str) -> Vec<LyricLine> {
    raw_plain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| LyricLine {
            time_ms: 0,
            text: line.to_string(),
        })
        .collect()
}

/// Construye un `LyricsPayload` priorizando `synced_lyrics` y haciendo fallback
/// a `plain_lyrics` (`synced: false`, `time_ms: 0`) si `synced_lyrics` viene vacío.
pub fn build_lyrics_payload(
    track_id: &str,
    synced_lyrics: Option<&str>,
    plain_lyrics: Option<&str>,
) -> LyricsPayload {
    if let Some(raw_synced) = synced_lyrics {
        let lines = parse_synced_lrc(raw_synced);
        if !lines.is_empty() {
            return LyricsPayload {
                track_id: track_id.to_string(),
                synced: true,
                lines,
            };
        }
    }

    if let Some(raw_plain) = plain_lyrics {
        let lines = parse_plain_lyrics(raw_plain);
        if !lines.is_empty() {
            return LyricsPayload {
                track_id: track_id.to_string(),
                synced: false,
                lines,
            };
        }
    }

    LyricsPayload {
        track_id: track_id.to_string(),
        synced: false,
        lines: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_synced_lrc_centiseconds_and_milliseconds_sorted() {
        let sample = "\
[00:15.250] Segunda línea
[00:04.50] Primera línea
[01:02.005] Tercera línea
";
        let payload = build_lyrics_payload("artist-song", Some(sample), None);
        assert!(payload.synced);
        assert_eq!(payload.lines.len(), 3);
        assert_eq!(payload.lines[0].time_ms, 4_500);
        assert_eq!(payload.lines[0].text, "Primera línea");
        assert_eq!(payload.lines[1].time_ms, 15_250);
        assert_eq!(payload.lines[1].text, "Segunda línea");
        assert_eq!(payload.lines[2].time_ms, 62_005);
        assert_eq!(payload.lines[2].text, "Tercera línea");
    }

    #[test]
    fn test_fallback_to_plain_lyrics_when_synced_empty() {
        let plain = "Verso uno\nVerso dos\n";
        let payload = build_lyrics_payload("artist-song", Some(""), Some(plain));
        assert!(!payload.synced);
        assert_eq!(payload.lines.len(), 2);
        assert_eq!(payload.lines[0].time_ms, 0);
        assert_eq!(payload.lines[0].text, "Verso uno");
        assert_eq!(payload.lines[1].time_ms, 0);
        assert_eq!(payload.lines[1].text, "Verso dos");
    }
}
