// SPDX-License-Identifier: MPL-2.0
//! ACadSharp modeler projection. Reader quirks stay here; native geometry,
//! payloads and display caches are never modified by the dump.

use super::{array, bool_value, color, inline, integer, number, points3, unmapped, vec3};
use crate::entities::solid3d::source::{AcisSource, DwgAcisSource, DxfAcisSource};
use crate::entities::solid3d::{AcisData, Silhouette, Wire};
use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use crate::io::dwg::DwgVersion;
use crate::types::{Color, DxfVersion, Vector3};
use serde_json::Value;

const ACIS_SIGNATURE_HEX: &str = "414349532042696e61727946696c65";
const END_MARKERS: [&[u8]; 4] = [
    b"End-of-ACIS-data",
    b"End-of-ASM-data",
    b"\x0e\x03End\x0e\x02of\x0e\x04ACIS\x0d\x04data",
    b"\x0e\x03End\x0e\x02of\x0e\x03ASM\x0d\x04data",
];

fn hex_byte(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 15) as usize] as char);
}

fn ascii_char(out: &mut String, c: char) {
    // Encoding.ASCII replaces each non-ASCII UTF-16 code unit with '?'.
    if c.is_ascii() {
        hex_byte(out, c as u8);
    } else {
        for _ in 0..c.len_utf16() {
            hex_byte(out, b'?');
        }
    }
}

fn cipher(c: char) -> char {
    if c > ' ' && c < '\u{9f}' {
        char::from(159 - c as u8)
    } else {
        c
    }
}

fn native_cipher(c: char) -> char {
    match c {
        ' ' | '\r' | '\n' | '\t' => c,
        '@'..='_' => char::from(159 - c as u8),
        c if c.is_ascii() => char::from(c as u8 ^ 0x5f),
        _ => c,
    }
}

fn dxf_text(data: &AcisData, source: &DxfAcisSource) -> (Value, Value) {
    let mut chars = source.layout.parts(&data.sat_data).flat_map(str::chars);
    let mut payload = String::new();
    let mut proprietary = String::new();
    let mut encoded = None;
    for chunk in &source.chunks {
        if chunk.new_line {
            chars.next();
        }
        // AppendAcisLine inserts a separator if the previous text is nonempty,
        // including for an empty source chunk. Continuations have no separator.
        if chunk.new_line && !payload.is_empty() {
            hex_byte(&mut payload, b'\n');
        }
        let mut nonempty = false;
        let mut emit = |raw: char| {
            nonempty = true;
            if encoded.is_none() && raw > ' ' {
                encoded = Some(!raw.is_numeric());
            }
            proprietary.push(raw);
            ascii_char(
                &mut payload,
                if encoded.unwrap_or(false) {
                    cipher(raw)
                } else {
                    raw
                },
            );
        };
        let mut patches = chunk.source_override.iter().flatten().peekable();
        let mut offset = 0;
        while offset <= chunk.char_count {
            if patches.peek().is_some_and(|patch| patch.offset == offset) {
                let patch = patches.next().unwrap();
                for _ in 0..patch.remove {
                    chars.next();
                }
                offset += patch.remove;
                for c in patch.insert.chars() {
                    emit(c);
                }
            } else if offset < chunk.char_count {
                if let Some(c) = chars.next() {
                    emit(if source.native_encoded {
                        native_cipher(c)
                    } else {
                        c
                    });
                }
                offset += 1;
            } else {
                break;
            }
        }
        if nonempty && encoded.is_none() {
            encoded = Some(false);
        }
        proprietary.push('\n');
    }
    (
        if payload.is_empty() {
            Value::Null
        } else {
            Value::String(payload)
        },
        Value::String(proprietary),
    )
}

fn native_wire(wire: &Wire) -> Value {
    inline(
        "ModelerGeometry.Wire",
        vec![
            ("AcisIndex", integer(wire.acis_index)),
            ("ApplyTransformPresent", bool_value(wire.has_transform)),
            ("Color", color(wire.color)),
            (
                "HasRotation",
                bool_value(wire.has_transform && wire.has_rotation),
            ),
            ("HasShear", bool_value(wire.has_transform && wire.has_shear)),
            ("Points", points3(wire.points.iter().copied())),
            (
                "Scale",
                number(if wire.has_transform {
                    wire.scale.x
                } else {
                    0.0
                }),
            ),
            ("SelectionMarker", integer(wire.selection_marker)),
            (
                "Translation",
                vec3(if wire.has_transform {
                    wire.translation
                } else {
                    Vector3::ZERO
                }),
            ),
            ("Type", integer(wire.wire_type as u8)),
            (
                "XAxis",
                vec3(if wire.has_transform {
                    wire.x_axis
                } else {
                    Vector3::ZERO
                }),
            ),
            (
                "YAxis",
                vec3(if wire.has_transform {
                    wire.y_axis
                } else {
                    Vector3::ZERO
                }),
            ),
            (
                "ZAxis",
                vec3(if wire.has_transform {
                    wire.z_axis
                } else {
                    Vector3::ZERO
                }),
            ),
        ],
    )
}

fn native_silhouette(sil: &Silhouette) -> Value {
    silhouette(
        sil.viewport_id,
        sil.target,
        sil.view_direction,
        sil.up_vector,
        sil.is_perspective,
        array(sil.wires.iter().map(native_wire)),
    )
}

fn silhouette(
    id: i64,
    target: Vector3,
    direction: Vector3,
    up: Vector3,
    perspective: bool,
    wires: Value,
) -> Value {
    inline(
        "ModelerGeometry.Silhouette",
        vec![
            ("ViewportId", integer(id)),
            ("ViewportTarget", vec3(target)),
            ("ViewportDirectionFromTarget", vec3(direction)),
            ("ViewportUpDirection", vec3(up)),
            ("ViewportPerspective", bool_value(perspective)),
            ("Wires", wires),
        ],
    )
}

/// Reject malicious counts before allocating; every item must fit in the
/// remaining bounded record even at its shortest possible encoding.
fn count(reader: &mut DwgBitReader, min_bits: i64) -> Option<usize> {
    let n = reader.read_bit_long();
    (n >= 0 && i64::from(n) <= reader.remaining_bits() / min_bits).then_some(n as usize)
}

fn read_wire(reader: &mut DwgBitReader) -> Option<Value> {
    let kind = reader.read_byte(); // Preserve all byte values, not just the native enum.
    let marker = reader.read_bit_long();
    let index = reader.read_bit_short().min(256);
    let acis_index = reader.read_bit_long();
    let n = count(reader, 6)?;
    let points = points3((0..n).map(|_| reader.read_3bit_double()));
    let transform = reader.read_bit();
    let (mut x, mut y, mut z, mut translation) =
        (Vector3::ZERO, Vector3::ZERO, Vector3::ZERO, Vector3::ZERO);
    let (mut scale, mut rotation, mut shear) = (0.0, false, false);
    if transform {
        x = reader.read_3bit_double();
        y = reader.read_3bit_double();
        z = reader.read_3bit_double();
        translation = reader.read_3bit_double();
        // ACadSharp consumes one BD; the native cache contains a 3BD scale.
        scale = reader.read_bit_double();
        rotation = reader.read_bit();
        let _reflection = reader.read_bit();
        shear = reader.read_bit();
    }
    reader.check_error().ok()?;
    Some(inline(
        "ModelerGeometry.Wire",
        vec![
            ("AcisIndex", integer(acis_index)),
            ("ApplyTransformPresent", bool_value(transform)),
            ("Color", color(Color::from_index(index))),
            ("HasRotation", bool_value(rotation)),
            ("HasShear", bool_value(shear)),
            ("Points", points),
            ("Scale", number(scale)),
            ("SelectionMarker", integer(marker)),
            ("Translation", vec3(translation)),
            ("Type", integer(kind)),
            ("XAxis", vec3(x)),
            ("YAxis", vec3(y)),
            ("ZAxis", vec3(z)),
        ],
    ))
}

fn read_wires(reader: &mut DwgBitReader) -> Option<Value> {
    let n = count(reader, 17)?;
    let mut wires = Vec::new();
    for _ in 0..n {
        wires.push(read_wire(reader)?);
    }
    Some(Value::Array(wires))
}

fn read_payload(reader: &mut DwgBitReader) -> Option<Value> {
    reader.read_bit();
    let version = reader.read_bit_short();
    reader.check_error().ok()?;
    let mut out = String::new();
    match version {
        1 => loop {
            let n = count(reader, 8)?;
            if n == 0 {
                break;
            }
            for _ in 0..n {
                let b = reader.read_byte();
                hex_byte(&mut out, if b > 32 && b < 159 { 159 - b } else { b });
            }
        },
        2 => {
            // Match AcisTextCodec's marker priority, even if a different marker
            // occurs earlier. Scan shared bits with a fixed window, then encode
            // only the chosen prefix: no copy of a large discarded tail.
            let start = reader.position_in_bits();
            let mut window = [0u8; 32];
            let mut ends = [None; 4];
            let mut seen = 0;
            while reader.remaining_bits() >= 8 {
                window.rotate_left(1);
                window[31] = reader.read_byte();
                seen += 1;
                for (i, marker) in END_MARKERS.iter().enumerate() {
                    if ends[i].is_none()
                        && seen >= marker.len()
                        && window[32 - marker.len()..] == **marker
                    {
                        ends[i] = Some(reader.position_in_bits());
                    }
                }
            }
            let end = ends
                .into_iter()
                .flatten()
                .next()
                .unwrap_or(reader.position_in_bits());
            reader.set_position_in_bits(start);
            while reader.position_in_bits() < end {
                hex_byte(&mut out, reader.read_byte());
            }
        }
        _ => return Some(Value::Null),
    }
    reader.check_error().ok()?;
    Some(if out.is_empty() {
        Value::Null
    } else {
        Value::String(out)
    })
}

struct DwgView {
    payload: Option<Value>,
    point: Value,
    wires: Value,
    silhouettes: Value,
}

fn dwg_view(source: &DwgAcisSource) -> Option<DwgView> {
    let mut reader = DwgBitReader::from_shared(
        source.data.clone(),
        DwgVersion::from_dxf_version(source.version).ok()?,
        source.version,
    );
    reader.set_end_bit(source.end_bit);
    reader.set_position_in_bits(source.start_bit);
    let mut result = DwgView {
        payload: None,
        point: vec3(Vector3::ZERO),
        wires: Value::Array(Vec::new()),
        silhouettes: Value::Array(Vec::new()),
    };
    if source.version < DxfVersion::AC1027 && !reader.read_bit() {
        result.payload = Some(read_payload(&mut reader)?);
        return Some(result); // The oracle never reads this body's wireframe tail.
    }
    if reader.read_bit() {
        if reader.read_bit() {
            result.point = vec3(reader.read_3bit_double());
        }
        reader.read_bit_long(); // isoline display count
        if reader.read_bit() {
            result.wires = read_wires(&mut reader)?;
        }
        let n = count(&mut reader, 23)?;
        let mut silhouettes = Vec::new();
        for _ in 0..n {
            let id = reader.read_bit_long_long();
            let target = reader.read_3bit_double();
            let direction = reader.read_3bit_double();
            let up = reader.read_3bit_double();
            let perspective = reader.read_bit();
            let wires = if reader.read_bit() {
                read_wires(&mut reader)?
            } else {
                Value::Array(Vec::new())
            };
            silhouettes.push(silhouette(id, target, direction, up, perspective, wires));
        }
        result.silhouettes = Value::Array(silhouettes);
        if !reader.read_bit() {
            result.payload = Some(read_payload(&mut reader)?);
        }
    }
    reader.check_error().ok()?;
    Some(result)
}

fn native_payload(data: &AcisData) -> Value {
    if data.has_data() {
        super::bytes(if data.is_binary {
            &data.sab_data
        } else {
            data.sat_data.as_bytes()
        })
    } else {
        Value::Null
    }
}

pub(super) fn properties(
    data: &AcisData,
    point: Vector3,
    wires: &[Wire],
    silhouettes: &[Silhouette],
) -> Vec<(&'static str, Value)> {
    let (payload, version, point, wires, silhouettes, proprietary) = match data.source.as_deref() {
        Some(AcisSource::Dwg(source)) => {
            if let Some(view) = dwg_view(source) {
                let payload = if source.version >= DxfVersion::AC1027 && !data.sab_data.is_empty() {
                    native_payload(data) // AcDs data is applied after entity construction.
                } else {
                    view.payload.unwrap_or(Value::Null)
                };
                (
                    payload,
                    0,
                    view.point,
                    view.wires,
                    view.silhouettes,
                    Value::String(String::new()),
                )
            } else {
                // Bounded replay cannot safely project a malformed oracle layout.
                (
                    unmapped(),
                    0,
                    unmapped(),
                    unmapped(),
                    unmapped(),
                    Value::String(String::new()),
                )
            }
        }
        Some(AcisSource::Dxf(source)) => {
            let (text, raw) = dxf_text(data, source);
            let payload = if data.is_binary && !data.sab_data.is_empty() {
                native_payload(data)
            } else {
                text
            };
            // DXF callbacks never populate display-cache properties.
            (
                payload,
                source.modeler_version,
                vec3(Vector3::ZERO),
                Value::Array(Vec::new()),
                Value::Array(Vec::new()),
                raw,
            )
        }
        None => (
            native_payload(data),
            data.version as i16,
            vec3(point),
            array(wires.iter().map(native_wire)),
            array(silhouettes.iter().map(native_silhouette)),
            Value::String(String::new()),
        ),
    };
    let binary = if payload == unmapped() {
        unmapped()
    } else {
        bool_value(
            payload
                .as_str()
                .is_some_and(|s| s.starts_with(ACIS_SIGNATURE_HEX)),
        )
    };
    vec![
        ("AcisData", payload),
        ("IsBinaryAcisData", binary),
        ("ModelerFormatVersion", integer(version)),
        ("Point", point),
        ("Wires", wires),
        ("Silhouettes", silhouettes),
        ("ProprietaryData", proprietary),
    ]
}
