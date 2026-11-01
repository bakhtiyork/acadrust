//! R7 (gate G7): regression tests for the findings of the mutation fuzzers (`Tools/fuzz/fuzz-dxf.py --impl rust`,
//! `fuzz-dwg.py --impl rust`). Each input is a minimised mutant of an MIT sample (or of a CadKit fuzz fixture
//! derived from one); the comment above each test names the finding (the panic location of the fuzz build, which
//! keeps debug assertions and overflow checks, like the test profile) and where the input came from. Every test
//! reads the input with the failsafe reader and must not panic: malformed input ends in an error or a notification.

use acadrust::{CadDocument, DxfReader, DxfReaderConfiguration};
use std::io::Cursor;

fn failsafe_dxf(text: &str) -> acadrust::Result<CadDocument> {
    DxfReader::from_reader(Cursor::new(text.as_bytes().to_vec()))
        .unwrap()
        .with_configuration(DxfReaderConfiguration {
            failsafe: true,
            ..Default::default()
        })
        .read()
}

/// panic@src/document.rs:5481 `attempt to add with overflow`: a handle of FFFFFFFFFFFFFFFF in the file left
/// `next_handle` at u64::MAX, and the allocator's `+= 1` overflowed (wrapped in a release build, as ACadSharp's
/// unchecked ulong does). Input: CadKit Tests/CadCoreTests/Fixtures/fuzz/handle-seed-overflow.dxf (a Swift-fuzz
/// minimisation of sample_AC1018.dxf).
#[test]
fn handle_of_u64_max_does_not_overflow_the_allocator() {
    let dxf = "  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n_ARCHTICK\n  0\nPOLYLINE\n  0\nVERTEX\n  0\nSEQEND\n  5\n\
FFFFFFFFFFFFFFFF\n  0\nENDBLK\n  0\nENDSEC\n  0\nEOF\n";
    let document = failsafe_dxf(dxf).expect("the failsafe read returns a document");
    let mut document = document;
    let first = document.allocate_handle();
    let second = document.allocate_handle();
    assert!(!first.is_null() && !second.is_null());
    // The counter cannot pass u64::MAX: the handles it issues are new ones, not the SEQEND's FFFFFFFFFFFFFFFF again.
    assert_ne!(first, second);
    for handle in [first, second] {
        assert_ne!(handle.value(), u64::MAX);
        assert!(document.get_entity(handle).is_none(), "{handle:?} is already in use");
    }
}

/// The same allocator while the file is read: entities without a handle after one of FFFFFFFFFFFFFFFF each get a
/// handle of their own (a saturating counter gave all of them FFFFFFFFFFFFFFFF).
#[test]
fn entities_after_a_handle_of_u64_max_get_distinct_handles() {
    let dxf = "  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  5\nFFFFFFFFFFFFFFFF\n  8\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n\
  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n2\n 21\n2\n  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n3\n 21\n3\n  0\nENDSEC\n  0\nEOF\n";
    let mut document = failsafe_dxf(dxf).expect("the failsafe read returns a document");
    let mut handles: Vec<u64> = document.entities().map(|e| e.common().handle.value()).collect();
    assert_eq!(handles.len(), 3, "{handles:X?}");
    handles.push(document.allocate_handle().value());
    let count = handles.len();
    handles.sort_unstable();
    handles.dedup();
    assert_eq!(handles.len(), count, "a handle was issued twice: {handles:X?}");
    assert!(!handles.contains(&0));
}

/// Writes `bytes` under the test scratch directory and returns the ACadSharp dump of the failsafe read of it (the
/// path `acad-dump dump` takes), so a panic of the read or of the dump fails the test.
#[cfg(feature = "acadsharp-compat")]
fn dump_of(name: &str, bytes: &[u8]) -> serde_json::Value {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("r7");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut out = Vec::new();
    acadrust::compat::acadsharp::write_file(&path, &mut out).expect("the dump is written");
    std::fs::remove_file(&path).ok();
    serde_json::from_slice(&out).expect("the dump is JSON")
}

/// panic@src/compat/acadsharp/legacy_identity.rs:311 `attempt to add with overflow` (exit 101): an R12 DXF whose
/// header handle seed is FFFFFFFFFFFFFFFF made the ACadSharp view's template allocator (`initial`, `cursor`) pass
/// u64::MAX; ACadSharp's `ulong` arithmetic is unchecked, so the view wraps. Input: 68 bytes minimised from a mutant
/// of sample_AC1009_ascii.dxf, `fuzz-dxf.py --impl rust --seed 1` (mutant 308).
#[cfg(feature = "acadsharp-compat")]
#[test]
fn r12_handle_seed_of_u64_max_does_not_overflow_the_view() {
    let dxf = b"  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1009\n  5\nFFFFFFFFFFFFFFFF\n";
    let dump = dump_of("r12-handle-seed-u64-max.dxf", dxf);
    assert!(dump.is_object());
}

fn table_rows(dxf: &str) -> (usize, usize) {
    let document = failsafe_dxf(dxf).expect("the failsafe read returns a document");
    let table = document
        .entities()
        .find_map(|e| match e {
            acadrust::EntityType::Table(t) => Some(t),
            _ => None,
        })
        .expect("the table is read");
    (table.rows.len(), table.columns.len())
}

/// hang@sample_AC1027_ascii (a 2.5 MB mutant took more than 10 s; the view of the read, 1,000,000 rows of five cells,
/// was the time): the row count of an ACAD_TABLE (group 91) was 1,000,000 where the file declares four row heights
/// (141) and no cells, and the reader allocated a row for every declared one. Input: the table of
/// sample_AC1027_ascii.dxf (ACadSharp, MIT) with group 91 changed from 4 to 1000000 (`fuzz-dxf.py --impl rust
/// --seed 2`, mutant 382, the changed value only).
#[test]
fn declared_table_row_count_is_not_allocated_without_rows_or_cells() {
    let head = "  0\nSECTION\n  2\nENTITIES\n  0\nACAD_TABLE\n  5\nAB\n100\nAcDbEntity\n  8\n0\n100\nAcDbBlockReference\n  2\n*T1\n\
 10\n0.0\n 20\n0.0\n 30\n0.0\n100\nAcDbTable\n 91\n";
    let tail = "\n 92\n         5\n141\n1.0\n141\n1.0\n141\n1.0\n141\n1.0\n142\n2.5\n142\n2.5\n142\n2.5\n142\n2.5\n142\n2.5\n\
  0\nENDSEC\n  0\nEOF\n";
    assert_eq!(table_rows(&format!("{head}4{tail}")), (4, 5));
    let started = std::time::Instant::now();
    assert_eq!(table_rows(&format!("{head}1000000{tail}")), (4, 5));
    assert_eq!(table_rows(&format!("{head}2147483647{tail}")), (4, 5));
    assert!(started.elapsed().as_secs() < 5);
}

fn within_seconds(limit: u64, started: std::time::Instant) {
    assert!(started.elapsed().as_secs() < limit, "the read took {:?}", started.elapsed());
}

/// panic@section_reader.rs:10218 `capacity overflow` (the std frame of `Vec::reserve`): LTYPE group 73 (element
/// count) is an i16, and a negative one was cast to `usize` for `reserve`. Input: the LTYPE table of a mutant of
/// sample_AC1018_binary.dxf and sample_AC1021_binary.dxf (`fuzz-dxf.py --impl rust --seed 2`, mutants 3846 and 4686),
/// written here as ASCII with the one changed value.
#[test]
fn negative_linetype_element_count_does_not_overflow_the_reserve() {
    let dxf = "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLTYPE\n 70\n1\n  0\nLTYPE\n  2\nDASHED\n 70\n0\n  3\ndash\n 72\n65\n 73\n-1\n 40\n0.0\n\
  0\nENDTAB\n  0\nENDSEC\n  0\nEOF\n";
    let document = failsafe_dxf(dxf).expect("the failsafe read returns a document");
    assert!(document.line_types.get("DASHED").is_some());
}

/// The same fault class as the HATCH finding below, in the sibling lists that read a count the file states (hatch
/// source-object handles 97, seed points 98, polyline vertices 93, spline knots / control points / fit points, DGN
/// stroke and symbol counts): a count of 2^31 - 1 followed by the end of the file stopped nothing, and the loops
/// pushed a default per count. Hardening from the audit of the finding, not a fuzz minimisation.
#[test]
fn counts_of_two_billion_end_with_the_data() {
    let head = "  0\nSECTION\n  2\nENTITIES\n  0\nHATCH\n  5\n2A\n100\nAcDbEntity\n  8\n0\n100\nAcDbHatch\n 10\n0.0\n 20\n0.0\n 30\n0.0\n\
210\n0.0\n220\n0.0\n230\n1.0\n  2\nSOLID\n 70\n1\n 71\n0\n 91\n1\n";
    for tail in [
        // 97 source boundary objects, then the end of the file
        " 92\n1\n 93\n0\n 97\n2147483647\n",
        // a polyline path with 2^31 - 1 vertices and one
        " 92\n2\n 72\n0\n 73\n1\n 93\n2147483647\n 10\n1.0\n 20\n2.0\n",
        // seed points
        " 92\n1\n 93\n0\n 97\n0\n 75\n0\n 76\n1\n 47\n0.1\n 98\n2147483647\n 10\n1.0\n 20\n2.0\n",
        // a spline path: knots, control points
        " 92\n8\n 93\n1\n 72\n4\n 73\n1\n 74\n0\n 94\n3\n 95\n2147483647\n 96\n2147483647\n 40\n0.0\n",
    ] {
        let started = std::time::Instant::now();
        let _ = failsafe_dxf(&format!("{head}{tail}"));
        within_seconds(5, started);
    }
}

/// hang@sample_AC1024_ascii (more than 10 s, `fuzz-dxf.py --impl rust --seed 2`, mutant 3006): HATCH group 97
/// (source boundary object count) changed from 1 to 2147483647 in sample_AC1024_ascii.dxf; the loop read a pair per
/// count and kept looping after the end of the file.
#[test]
fn hatch_source_object_count_stops_at_the_end_of_the_file() {
    let dxf = "  0\nSECTION\n  2\nENTITIES\n  0\nHATCH\n  5\n2A\n100\nAcDbEntity\n  8\n0\n100\nAcDbHatch\n 10\n0.0\n 20\n0.0\n 30\n0.0\n\
210\n0.0\n220\n0.0\n230\n1.0\n  2\nSOLID\n 70\n1\n 71\n0\n 91\n1\n 92\n1\n 93\n0\n 97\n2147483647\n";
    let started = std::time::Instant::now();
    let _ = failsafe_dxf(dxf);
    within_seconds(5, started);
}

/// The same finding as the ASCII test above, as the fuzzer minimised it: 81 bytes of binary DXF (mutant 3846 of
/// `fuzz-dxf.py --impl rust --seed 2` on sample_AC1018_binary.dxf; the 16-bit value of group 73 is FFFF).
#[test]
fn negative_linetype_element_count_in_binary_dxf() {
    let bytes: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0\x45\x43\0\0\0SECTION\0\x02\0TABLES\0AB\0\0\0TABLE\0\x02\0LTYPE\0cD\0\0\0LTYPE\0\0\x31\0I\0\xff\xff";
    let _ = DxfReader::from_reader(Cursor::new(bytes.to_vec()))
        .unwrap()
        .with_configuration(DxfReaderConfiguration {
            failsafe: true,
            ..Default::default()
        })
        .read();
}

/// Runs `body` on its own thread and fails the test when it has not finished within `seconds` (a loop that never
/// ends must fail the test, not hang the run).
fn finishes_within<T: Send + 'static>(seconds: u64, body: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(body());
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(seconds))
        .unwrap_or_else(|_| panic!("did not finish within {seconds} s"))
}

/// The dump of a patched copy of a fixture (`patches` are (offset, byte)), cut to `len` bytes when given.
#[cfg(feature = "acadsharp-compat")]
fn dump_of_patched(name: &str, fixture: &[u8], patches: &[(usize, u8)], len: Option<usize>) -> serde_json::Value {
    let mut bytes = fixture.to_vec();
    for &(offset, value) in patches {
        bytes[offset] = value;
    }
    if let Some(len) = len {
        bytes.truncate(len);
    }
    let name = name.to_string();
    finishes_within(120, move || dump_of(&name, &bytes))
}

/// hang@BLOCKLOOKUPPARAMETER (and the same cell of the other dynamic-block samples), the view of the registration
/// order never ended: `registration_order::apply` skipped occupied handles with `saturating_add`, which stops at
/// u64::MAX, so a retained record holding handle FFFFFFFFFFFFFFFF kept the loop on one value forever. ACadSharp's
/// unchecked `ulong` wraps; so does the view now. Input: ACadSharp's BLOCKLOOKUPPARAMETER.dwg (the fork's
/// tests/fixtures/dynamic-blocks/lookup.dwg is the same file), three bytes changed (`fuzz-dwg.py --impl rust
/// --seed 1`, mutant 47962 after minimisation: 11091 -> 01, 11092 -> 00, 34234 -> 80).
#[cfg(feature = "acadsharp-compat")]
#[test]
fn registration_order_ends_when_the_handle_space_is_full_lookup() {
    let dump = dump_of_patched(
        "lookup-handle-max.dwg",
        include_bytes!("fixtures/dynamic-blocks/lookup.dwg"),
        &[(11091, 0x01), (11092, 0x00), (34234, 0x80)],
        None,
    );
    assert!(dump.is_object());
}

/// hang@export_sample, the same loop: ACadSharp's svg/export_sample.dwg cut to 81,484 bytes with four bytes at
/// 25636 changed to FF FF FF 7F (`fuzz-dwg.py --impl rust --seed 1`, mutant 3369 after minimisation).
#[cfg(feature = "acadsharp-compat")]
#[test]
fn registration_order_ends_when_the_handle_space_is_full_export_sample() {
    let dump = dump_of_patched(
        "export-sample-handle-max.dwg",
        include_bytes!("fixtures/svg/export_sample.dwg"),
        &[(25636, 0xFF), (25637, 0xFF), (25638, 0xFF), (25639, 0x7F)],
        Some(81484),
    );
    assert!(dump.is_object());
}

// ---- DWG findings (R2007 file header): minimised mutants of sample_AC1021.dwg (ACadSharp, MIT), as hex ----

/// 1,557 bytes: `fuzz-dwg.py --impl rust --seed 1`, mutant 12969, the first bytes of sample_AC1021.dwg with a page-map entry changed.
const AC1021_PAGE_MAP_OFFSET: &str = concat!(
    "41433130323100000000003203200d000021ff1e000021ff0000000000000000800c00000000000080000000c0720000e075",
    "0000020200007b01000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000004e0280ab106f3839094c58e4c3007f9f010235c70207",
    "1450087c687cf0010aef25954a7ff9e500e10687819819b67620da087f2f02019e2a40f887776b41006b02025797f8d2302a",
    "a4c18400778a00806f004d17006faf000959002800008f59201060007709000104022777007709700201703077028001106f",
    "0d39097758e400007f02010205c7028f1450297c687ff0016def250c4a7f28e500c40687859819ff762060087f090201242a",
    "407787770941002302027797f802302a0bc184aa778a68806f004d17006fafc20959b02800bf8f59fa1060bd77097e0104d2",
    "2777237709870201203077028001016f0da409772ce400ce7f0264020554028f94502960687f00016dd6250c817f286800c4",
    "6787851019ff0f2060017f093c0124c54077f477090300234802778bf802532a0b7c84aa8e8a68af6f0083170021afc28959",
    "b00800bf7059fa0560bd06097e4204d2417723c20987f20120157702d70101650da49c772c0200ce000264000554198f9448",
    "2960e77f00a46dd6280c81e92868b5c467cb8510f9ff0f006001ba093c3c24c54077f4a30903b6234892778b0e0253bb0b7c",
    "3aaa8eee68af260083c10021a0c28904b00804bf708afa05d3bd062a7e429bd2419723c26d87f29020156502d7b001658ca4",
    "9c522c027dce001b6400a3544ede94abc0603846004c82d6c308819fcb6835a96707821008020f7c23010aa03c95f7c5f932",
    "f4e1dc0381e848b6598bdac5532f097c9e378ef831af6b6c836b372157fe89d21508a4e770009405001e0600234200d44100",
    "a4c200e0f2005a152007d700c66500649c028e0200dd0070cc00702e4e027eab107a3839f54c582bc3006f9f019e35c78807",
    "1487087c247cf0680aef23954acaf9e5b0e106238198e6b6767ada08e12f02fb9e2a71f887096b41fd6b02025797aed23032",
    "a4c1f90077850080f6004d09006f2300092a002829008f942010e100776d0001e40227db0077607002ff7030402fa47df9c0",
    "161e506168f9a283c822b146923741c4d51abc8fdf6ae9cb4cdc7b6ec0046e7e946f02bbbc192884dc2a901948e7a428e9b5",
    "cbf900ba3c40a3b6920ebb3aee26c1a004048ad32a9b976d9065b08c527d1ba3dec0468208cba9820223a0f732dce859c509",
    "37316c37fe15e7941e23d4a4e05a07c6648eddcc2e7e7af52b6f9e8887246823cab023e67ae1fb7109fd02ae32f985f60923",
    "2a2994e16de4db60ff4061360e5e9e3f5e9a5273a318eeda9563da81c30288b818edbaa411a59481363ef22e86d2adcbff9c",
    "6c17d8e3f1b7d6894d2344d25e23456a5478d0997e843f8ad334f93bc6ca2b76c8b21abe3c0a798c2ec578d3d28fed437c3f",
    "35a3e160a5ba2d5f2f98941bfa97b7eb155ca19ce59b504a521241b016585a1e59975d24436e10a6a88762034c41691fa149",
    "43b7205000a0a0780001687768090027020901050366ff03048f0003da777f0006026002017f0000016520205000a0a07876",
    "01687768090327020901050166ff03048f0003da777f0000026002017f6800016520200900a0a078760768776809037f0209",
    "0105010aff03048f0050da777f0000c06002017f6868016520200901a0a078760708776809037f8f090105010a0103048f00",
    "5054777f0000c06802017f686801652020090109a0787607087f6809037f8f020105010a01fe048f005054207f0000c06868",
    "017f6868010120200901090a787607087f8f09037f8f020105010a01fef18f00505420680000c06868017f686801010b2009",
    "01090a7f7607087f8f02037f8f02017a010a01fef1a000505420686800c068680101686801010b0c0901090a7f7f07087f8f",
    "02027f8f02017ab70a01fef1a060505420686868c06868010101fef1a060505420686868c068680101016801010b0c0d0109",
    "0a7f7f7f087f8f0202028f02017ab7f801fef1a0602054206868686868680101010101010b0c0d0e090a7f7f7f8f7f8f0202",
    "020102017ab7f8",
);

/// 2,476 bytes: `fuzz-dwg.py --impl rust --seed 1`, mutant 105, the first bytes of sample_AC1021.dwg with the compressed header changed.
const AC1021_HEADER_DECOMPRESS: &str = concat!(
    "41433130323100000000003203200d000021ff1e000021ff0000000000000000800c00000000000080000000c0720000e075",
    "0000020200007b01000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000000000004e0280ab106f3839094c58e4c3007f9f010235c70207",
    "1450087c687cf0010aef25954a7ff9e500e10687819819b67620da087f2f02019e2a40f887776b41006b02025797f8d2302a",
    "a4c18400778a00806f004d17006faf000959002800008f59201060007709000104022777007709700201703077028001106f",
    "0d39097758e400007f02010205c7028f1450297c687ff0016def250c4a7f28e500c40687859819ff762060087f090201242a",
    "407787770941002302027797f802302a0bc184aa778a68806f004d17006fafc20959b02800bf8f59fa1060bd77097e0104d2",
    "2777237709870201203077028001016f0da409772ce400ce7f0264020554028f94502960687f00016dd6250c817f286800c4",
    "6787851019ff0f2060017f093c0124c54077f477090300234802778bf802532a0b7c84aa8e8a68af6f0083170021afc28959",
    "b00800bf7059fa0560bd06097e4204d2417723c20987f20120157702d70101650da49c772c0200ce000264000554198f9448",
    "2960e77f00a46dd6280c81e92868b5c467cb8510f9ff0f006001ba093c3c24c54077f4a30903b6234892778b0e0253bb0b7c",
    "3aaa8eee68af260083c10021a0c28904b00804bf708afa05d3bd062a7e429bd2419723c26d87f29020156502d7b001658ca4",
    "9c522c027dce001b6400a3544ede94abc0603846004c82d6c308819fcb6835a96707821008020f7c23010aa03c95f7c5f932",
    "f4e1dc0381e848b6598bdac5532f097c9e378ef831af6b6c836b372157fe89d21508a4e770009405001e0600234200d44100",
    "a4c200e0f2005a152007d700c66500649c028e0200dd0070cc00702e4e027eab107a3839f54c582bc3006f9f019e35c78807",
    "1487087c247cf0680aef23954acaf9e5b0e106238198e6b6767ada08e12f02fb9e2a71f887096b41fd6b02025797aed23032",
    "a4c1f90077850080f6004d09006f2300092a002829008f942010e100776d0001e40227db0077607002ff7030402fa47df9c0",
    "161e506168f9a283c822b146923741c4d51abc8fdf6ae9cb4cdc7b6ec0046e7e946f02bbbc192884dc2a901948e7a428e9b5",
    "cbf900ba3c40a3b6920ebb3aee26c1a004048ad32a9b976d9065b08c527d1ba3dec0468208cba9820223a0f732dce859c509",
    "37316c37fe15e7941e23d4a4e05a07c6648eddcc2e7e7af52b6f9e8887246823cab023e67ae1fb7109fd02ae32f985f60923",
    "2a2994e16de4db60ff4061360e5e9e3f5e9a5273a318eeda9563da81c30288b818edbaa411a59481363ef22e86d2adcbff9c",
    "6c17d8e3f1b7d6894d2344d25e23456a5478d0997e843f8ad334f93bc6ca2b76c8b21abe3c0a798c2ec578d3d28fed437c3f",
    "35a3e160a5ba2d5f2f98941bfa97b7eb155ca19ce59b504a521241b016585a1e59975d24436e10a6a88762034c41691fa149",
    "43b7205000a0a0780001687768090027020901050366ff03048f0003da777f0006026002017f0000016520205000a0a07876",
    "01687768090327020901050166ff03048f0003da777f0000026002017f6800016520200900a0a078760768776809037f0209",
    "0105010aff03048f0050da777f0000c06002017f6868016520200901a0a078760708776809037f8f090105010a0103048f00",
    "5054777f0000c06802017f686801652020090109a0787607087f6809037f8f020105010a01fe048f005054207f0000c06868",
    "017f6868010120200901090a787607087f8f09037f8f020105010a01fef18f00505420680000c06868017f686801010b2009",
    "01090a7f7607087f8f02037f8f02017a010a01fef1a000505420686800c068680101686801010b0c0901090a7f7f07087f8f",
    "02027f8f02017ab70a01fef1a060505420686868c068680101016801010b0c0d01090a7f7f7f087f8f0202028f02017ab7f8",
    "01fef1a0602054206868686868680101010101010b0c0d0e090a7f7f7f8f7f8f0202020102017ab7f8fcfef1a06020682068",
    "6868680168010101010f010b0c0d0e8f0a7f7f7f8f018f02020201e9017ab7f8fc68f1a06020680168686868011001010101",
    "0f7f0b0c0d0e8f027f7f7f8f010302020201e9007ab7f8fc6860a060206801516868680110110101010f7f8f0c0d0e8f0201",
    "7f7f8f010310020201e9006fb7f8fc68600160206801511268680110117f01010f7f8f020d0e8f02014f7f8f010310c00201",
    "e9006f68f8fc68600101206801511213680110117f8f010f7f8f02080e8f02014f788f010310c07101e9006f6814fc686001",
    "017f6801511213020110117f8fe20f7f8f0208208f02014f7868010310c07101e9006f681415686001017f7f015112130202",
    "10117f8fe2ab7f8f0208206002014f7868680310c0710101006f681415166001017f7f8f511213020208117f8fe2ab7f8f02",
    "08206061014f7868681710c07101017f6f681415160201017f7f8f881213020208807f8fe2ab7f680208206061014f786868",
    "1718c07101017f7f681415160202017f7f8f880a1302020880008fe2ab7f686f0820606101317868681718197101017f7f8f",
    "1415160202117f7f8f880af5020208800068e2ab7f686f0120606101311a68681718197f01017f7f8f021516020211d07f8f",
    "880af540020880006868ab7f686f0101606101311a1b681718197f7f017f7f8f020216020211d06c8f880af540a008800068",
    "68687f686f0101016101311a1b1c1718197f7f7f7f7f8f020200020211d06c8f880af540a0a980006868681d686f010101ff",
    "01311a1b1c0118197f7f7f1e7f8f0202008f0211d06c8f000af540a0a978006868681d996f010101ff1f311a1b1c018f197f",
    "7f7f1e008f0202008f7811d06c8f0079f540a0a978206868681d99ff010101ff1f111a1b1c018f217f7f7f1e008f0202008f",
    "7800d06c8f00797840a0a97820e968681d99ff220101ff1f118f1b1c018f21007f7f1e008f7802008f7800416c8f00797823",
    "a0a97820e9ff681d99ff220101ff1f118f241c018f21008f7f1e008f7800008f780041788f00797823f1a97820e9ff281d99",
    "ff22017fff1f118f2400018f21008f001e008f7800008f780041780000797823f1007820e9ff281999ff22017f481f118f24",
    "00e78f21008f00a4008f780000287800417800e9797823f100b520e9ff2820cbff22017f00f9118f2400000021008f0003ba",
    "8f780000003c0041780006407823f10000a3e9ff282050b622017f0001928f240000270e008f000366bb78000000033a4178",
    "000602ee23f100000026ff28205000c1017f000168a024000027",
);

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn read_dwg_bytes(bytes: Vec<u8>) {
    let _ = acadrust::DwgReader::from_stream(Cursor::new(bytes))
        .read();
}

/// panic@src/io/dwg/dwg_reader.rs:2475 `attempt to add with overflow` (the running offset of the R2007 page map
/// summed 64-bit page sizes of the file) and the sibling AC21 header paths on the same input.
#[test]
fn ac1021_page_map_with_huge_page_sizes_does_not_overflow() {
    finishes_within(60, || read_dwg_bytes(unhex(AC1021_PAGE_MAP_OFFSET)));
}

/// panic@src/io/dwg/decompressor_ac21.rs:468 `index out of bounds` (and the same function's reads and copies at
/// 447, 453, 454, 460, 461, 469, 470, 471 on the larger mutants): the R2007 LZ77 decoder indexed its source and
/// destination with offsets and lengths read from the stream.
#[test]
fn ac1021_header_with_a_damaged_compressed_block_is_an_error() {
    finishes_within(60, || read_dwg_bytes(unhex(AC1021_HEADER_DECOMPRESS)));
}

/// panic@src/io/dwg/dwg_stream_readers/handle_reader.rs:101 and :132 `attempt to shift left with overflow` (minimised
/// mutants of BLOCKVISIBILITYPARAMETER.dwg and BLOCKROTATIONPARAMETER.dwg, `fuzz-dwg.py --impl rust --seed 1`,
/// mutants 82313 and 22851): a handle-map entry of more than nine continuation bytes shifted a u64 by 64 or more.
/// ACadSharp's `ulong <<` masks the count; the reader does the same. The input is the chunk of the handle section
/// the mutants changed, reduced to its cause: one chunk of thirteen 0xFF bytes.
#[test]
fn modular_chars_of_more_than_nine_bytes_do_not_overflow_the_shift() {
    use acadrust::io::dwg::dwg_stream_readers::handle_reader::read_handles;
    let mut chunk = vec![0x00, 0x0F];
    chunk.extend([0xFF; 13]);
    let _ = read_handles(&chunk).expect("a damaged chunk is read, not rejected");
}

/// panic@src/io/dwg/dwg_stream_readers/classes_reader.rs:73 `attempt to add with overflow`
/// (`fuzz-dwg.py --impl rust --seed 1`, mutant 1380 of sample_AC1014.dwg): the size field of the Classes section
/// is a signed 32-bit number and was cast to usize, so -1 became 2^64 - 1. Input: the sentinel and the field.
#[test]
fn negative_classes_section_size_is_an_error() {
    use acadrust::io::dwg::dwg_stream_readers::classes_reader::read_classes;
    use acadrust::io::dwg::file_headers::section_definition::start_sentinels;
    use acadrust::types::DxfVersion;
    let mut data = start_sentinels::CLASSES.to_vec();
    data.extend([0xFF, 0xFF, 0xFF, 0xFF]);
    data.extend([0u8; 40]);
    assert!(read_classes(&data, DxfVersion::AC1015, 0).is_err());
    assert!(read_classes(&data, DxfVersion::AC1014, 0).is_err());
}

/// panic@src/io/dwg/decompressor_ac18.rs:29 `capacity overflow` (`fuzz-dwg.py --impl rust --seed 1`, mutant 38 of
/// sample_base/empty.dwg): the output size is the page header's number (a negative i32 cast to usize in the
/// reader) and the output vector reserved it. It now reserves what the source can plausibly expand to.
#[test]
fn ac18_output_size_from_a_damaged_header_does_not_reserve_it() {
    use acadrust::io::dwg::decompressor_ac18::decompress_ac18;
    let source = [0x01, 0x41, 0x42, 0x43, 0x44, 0x11];
    assert_eq!(decompress_ac18(&source, usize::MAX).len(), 4);
    assert_eq!(decompress_ac18(&source, (-1i32) as usize).len(), 4);
    assert_eq!(decompress_ac18(&source, 1 << 47).len(), 4);
}

/// The R2007 decoder against damaged streams (the class of panic@src/io/dwg/decompressor_ac21.rs:447-471): a valid
/// stream, then every truncation and 20,000 deterministic byte changes (an LCG with seed 7), each decoded into a
/// buffer of the original size: Ok or Err, never a panic or a write outside the buffer.
#[test]
fn ac21_decoder_never_panics_on_damaged_streams() {
    use acadrust::io::dwg::compressor_ac21::compress_ac21;
    use acadrust::io::dwg::decompressor_ac21::decompress_ac21;
    let mut original = Vec::new();
    for i in 0..600u32 {
        original.extend_from_slice(&(i % 7).to_le_bytes());
        original.extend_from_slice(b"abcabcabcabcabc");
    }
    let compressed = compress_ac21(&original);
    let mut padded = compressed.clone();
    padded.extend([0u8; 64]);
    let mut output = vec![0u8; original.len() + 64];
    decompress_ac21(&padded, 0, compressed.len() as u32, &mut output).expect("the undamaged stream decodes");
    assert_eq!(&output[..original.len()], &original[..]);

    for cut in 0..compressed.len() {
        let mut out = vec![0u8; original.len() + 64];
        let _ = decompress_ac21(&compressed[..cut], 0, compressed.len() as u32, &mut out);
    }
    let mut state = 7u64;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    for _ in 0..20_000 {
        let mut damaged = padded.clone();
        for _ in 0..1 + next() % 3 {
            let at = next() % compressed.len();
            damaged[at] = next() as u8;
        }
        let mut out = vec![0u8; if next() % 4 == 0 { 20 } else { original.len() + 64 }];
        let _ = decompress_ac21(&damaged, 0, compressed.len() as u32, &mut out);
    }
}

/// 23913 bytes of raw deflate (the mutant is 30639 bytes): `fuzz-dwg.py --impl rust --seed 1`, mutant 9322, sample_base/empty.dwg
/// (AC1032, ACadSharp, MIT) with a handful of bytes changed so that a section map states a negative page count.
const AC1032_NEGATIVE_PAGE_COUNT: &str = concat!(
    "ecbc795c4cedff3f7e9d334bd32c35ed7b9dce4c69d72eada79ad2caa8899649534da914a910c251510821348410d2edc64d",
    "c87233f61042082164dfbac9beccf73a83f7fb7d7fbeefcff7f37d7cbebfc7f78fdfef77e6bece9ceb3aafe5f97a5ddbeb75",
    "35eee05037570f77a0be684a04006b6009fee5c2901fdf242cbd0c78d3040045013084ed7de047f93fb972888f1f963aec79",
    "6d7c9bf3d75f079f3ebfbbf9967f053750211cc33367ee1b7667b76e6c7bc8c1059b79b4e419f3426dac52bbce7dbd5a1ee6",
    "a7c79bc736bfb5dddd7595f895acd4fbdbd248f7f7776644f61c19a1ffa6cf2bbb73dc09d77d02bff5781fbe64cdbd055637",
    "677e5edef1313c8fbea258eb86e1e0b1395d1fa2a23a47e74c168fd2001971f2e0e7b09c8365dccf923c66df7a61f29cbb94",
    "f9ffb1b04002280693403a2802b960c2bfbca1aefe040002aa6d40695b248dfaced18aa2fdf7fdd30ff11da5b08d821847fd",
    "c04815fce89c26a3db3b2aac6c0a34babcedecd68c3e1bb446c477bc0675a1c86308460cb99943605715d3c0ffe3d73cf1f0",
    "613cb6291b3ef2222344713fcd4f6151ba7e5bfa5b171c4d401c2309b3b67355a954f0fe9fcbf200201b0005001d00a8a89a",
    "8707f0c8061e0ae0d1013c54d4cb6c0f909d0db21520bb0364ab285a85075064038502283a804245b17678808e6cd0a1001d",
    "1da04305250195075065039502a83a804a4509a6444359901952ab283dd495ede1a1f0f0e8f0f050516a612d3bdb235be191",
    "dde191ada250c0978a6c0f85c243d1e1a15051a0206d47b64787c2a3a3c3a343053142560f55b6874ae1a1eaf050a928c894",
    "0d1034440961a9280b284d5034940599559441541d5e8aecec8eec6c15651f540b6b0a45b6a2235ba1a2cc8528e0cb0e4576",
    "474776870a5a0f4141da6c95225bd591ad5251cea09c05b9a13ba0fd2aca3794495018040d51aa2857518aa16c281aca5251",
    "9ea35ec33abc3a140a15e548681f540b6b1d1d8a0e15f42b3417a2802f15aa0e854a45b999ea956c8a9672b48af23ae5bb6c",
    "8a9572878aea04cac26c4a12055a45f50985239b124c8956515d445167537aa84b057b0c3a12da07d5c25a071c34ead1a052",
    "f7b74adda32a759fa9d4bda252fb5da5f6ac4aed3b95da3b2ab5fd2ab5852ab50d2a354a951a874aad49a596a55253ab7ed4",
    "e130e6f14c4d4dedececbcbcbc424343c562716a6aeab871e34a4b4be7ce9dbb6cd9b2f5ebd76fdfbefde0c18367ce9cb976",
    "ed5a5f5fdf5f7ffdf583f77f71b98489ffa2be2245c192d29b2f8f223c583bd6f8614481d690c67fcfd27b0ca4a400e4b2de",
    "da5aaa1a19365cb43d4456de7b73e947c733d7af67347b29330ebeeefa415c1ba7011ae1baf05b823c78012cbf8dfa51426f",
    "cf1cf2feaef6586a3ef2413028849f48301e0ecb09400457ae125862e0ea550c9fe08a014b5b67bfcdfaebcb22777d5efdc9",
    "6ec2fa6f45906b127c3b01c84116a41b0730c8391a0c032ef009031290a3e6c7a0cc5c900f69b09ff574f5db22c84d49cf82",
    "fcbf38314897ae6ec5e03d1d4cfef936034c55f38d87f7ff596bba1a7d3e949ea9464eadb914e504a8e3dfd1ffa0cc8234c5",
    "3fe5ff7b092ed0ea27bd2b469ebcb04bd47ce4fd13bf396b44ddc01fae9c456a79932075c93fbc56040afea67d3cac15a8ad",
    "0e005280ff0347287c12a95b28bb26a97d2307696a5b8b20a21ffc3f78241083175c965c80abbad88152e00d3c81fd4ffe22",
    "c833f6672f15411ffd7b29ee90c305de7ff0e4aaeda6fa371f7ed2609dd2fe4302f56eec3ff0fe40ea0c35ba41dd6ec017ca",
    "710543ff53ddf95067a65a6e16f48ae8a71c8a172ea43fb9288e12489b09690be0e7478dc2f0833a1deaf0506bf3841f6fe0",
    "03df78c1e2095bdca15c6fe86b4f758b1794910986c0ef2190de555dcb80cf99ea3e4d87cfd9c0fdbfdca876a66980aa1438",
    "3752e1bc806541ca8f3235f6dab29bc36784c7dff1b178116a7d7685c9bdfb46a60e63fd6eccfae07ff002ab31befbe5e585",
    "274f83ffadf94387c5ac3be26d073b30f6c01f17677f6815efe6feec17aa673dfff1e4faffd999d63b67dd9b4f2372f8bf2d",
    "6101c741bb6f0c47fc212535d30ad5e3334b8d351b8e997f8e181c4c57f7f25078f78123210bfacf19fa126e9db0cd593df6",
    "a831e4ac1e0f4361dd19b6f8a8c78b3b6c7187e325038e37397c37034a0b04ffd489fd4387cfcf37ff3a4702ff438ce70f06",
    "ff87f754cb2f59ff5e2e85ee8764b9da17596a7f506b46204446217457e3f552df87c29e7483cfbef03e543d17bdd43afe67",
    "deff5ab33be4fdf756fd7d95c27ee2f0f86fd9476919fa6fb5fcc775edbfe73df7ff03effdb2e9bff6dee0ff691406fefffb",
    "c2ffebf7855f97385103644be5c13d49705f80659cf447f158a835bda3d6d18a93975e24276176c35287498500e552d97029",
    "aa0dc27e2efcc1b08860094d65f0c40866c305cc6d578bb3c6e6ca4b89f97a24ad10956225e82424810425348c6d0098656c",
    "1a09f32552e3105a8262002cd3070d0ff3cf881fad79a659b30b8142c9850c907a8851c668e095d05a11c6b6abf2f4122850",
    "fa41c268d0f4fec643c49a54c615528324218598290090a4b0687271fa64815447ac471a7b7f45dcffd4e5ac2320156eae93",
    "18c4f4874f41b0044af5318743ba986933cde309c0bfea21e945993978be41a203a6f195511d99c8cd070db45404cfa283c4",
    "59956829e20e5028bf383387821023d1c41bf4a58854478a34a4db7c3248a85e96c40463a0256526a9a8cc866b224591224a",
    "af9d512a5aa9aed357c03af72f76be7683b99421a64bcda5a8ad93692aca3faf63bc50a75809a42c400fce1465a449722615",
    "648c4fcfcd4f8bcfccc92a4807120660888a7d7d254559e92559f2e0e2111979599925a2f49274e74a408bc91a9b9e39d559",
    "0c3891e3e559a5597271d184c2aca2129712c08d481f2fcfcf0a2e2929cacd98549255a3e54e320262c08434e84ea99b9470",
    "f9c6d08aa78e351ad2a4e1d248777390c8b6e55a95640dd01352d141755692b1520d2948d0d716a395c542274cc21033ca10",
    "29a7046d30944e68a54b841237bcd642c2a0c1ee282e2ec63f5aaccc881481d65c69ba6f0d731bb3646c32add8579685a467",
    "e46725d13224539cad9920a08d31b894ea330a487ada207f3db197b4591c5803245e6bd2f42b231aba9358194a804b0de93f",
    "73edc414549d6a2f8a5ec24a142366f75630134fb1ff96634b743ce1e8e4fd1709b55883f3f77c5acae6fc3d9d4ed4025aff",
    "553a2d364c34d4f8672e2d35e1fc3d954eb4e0fc3d93965873fe9e488b6d80eeff4e223dc6353b5b628f5059b4c475946976",
    "b65417a112e8784f285c0ce83fb367e910cedf93e764bf0e0fb106d0fc5fa4ce63867b28460d5328528354d2f064cf6c85d8",
    "19a1f2e5c4a838cb6c45a22142a5ca92e1c91af00d40a82c392eaec323d18212fa9fe6c871b91e1dc9e98a8e91c92a496aea",
    "f0ec44ada450557c3a3433d913b63aabc6c8151ea35da180244355720e04e929a501fabfcf82c58539b324d6f21269b12c57",
    "6c939f9a3825274e6a271f26992ef34f74ca778d9bd5e1911cd5a1101be619aa54a372552aa9678e29b0fdefe6b389f69e89",
    "9c1286d6cfbcd5ef57deea86f7f019c71adf8d28e00c6e2c65f9eb038d0bcc254b0092bf2ac54caac9f895a50eaed51e4a00",
    "290f03ff772e772ee0fdc7361d00ec05872befe55fdb1eb7335403540c93071bc1120dcb829fe54da86ad19af07e57e62f26",
    "a416fdbf85f96fd760f37f1e1dfe1b23fa43344047b83c9881c883cf69c112fea3ecce1cbca4423e9203c08cb3d0013c10c0",
    "05441e114177dc2c78e46845dfc7055e5c926826229884db09dc6a56020738f300f18ca8a6e342bf75758990450b04532cf8",
    "297c2e7e848c63029e36086ea9b6dab3887a24839fc1c74a4d60c107210e90ce7d9d30d9082c44f864485e703b1d17f935f9",
    "9df0ebf3a713581fe179f78f598ec08caf846c247f804f77a5f3e904a39fde4957d2c5d4335dccc01832eaaeddc86645558d",
    "b47453d3e7b6903ac27fd2f7d2f90c19a31f52ba429e5a862b0340097c462dda680ab9aef1f820560784b41064355dd8e7ef",
    "207ce61fb4b2cb0d5d8eb2c95087d04e3e7d2a1de12274848f94c16f2142c06f028940f4914458f3472448cccf167f482381",
    "34625817223e88316c61ff8a7fd2e0f70898e38d0071208c4ff0957c4207182975800e6100f8842ea1070c485d02d694fa4a",
    "036008eb3a04bf696720af9081d1fbe13e01e8ce6a2432a1962bc42357eb16233c4a43a929a0c7423d121846c4c1a829186a",
    "1289292d32739a9e5a961465ea10faa45af824fa409ba93451ab16ba470c5d42306ad56e0ddb69013408c4158a1643b3f4e1",
    "5dac36c51cd6747762802d569b2001f1b024c1e730301c2a8b85dfa1b52db8be526c932840f4802e918ceb481aad019de10a",
    "e5420df45a3aec0cd80db55063e15161c04e21570ccd60416dfa50ba189167a1fc9d02fa28083f1e24c843760a817e582d61",
    "0c7d644c1a90c68439694a1813c6a4193081be83cf4a0b7837539ae5ff98767e968466a51d9df06c7291135e72eb2681c845",
    "e4db87f70984d627bc4f105e74d7131e42a7bbfeb26090c026a54d4c0276af050be98328762287901a641ba2110283d01818",
    "f84d5587c0c14b9466c04c690cd59b012332c7badabad2bdcffd8487d5a175be60189b9cd4c7246af9747b3ad2063f87904e",
    "a4129626a41ba94358f1ea0079bc3a902ca2c24fa4712e3697ffa6f6c52c033718d3b2c930c82ee6d3ad63d48c6d482dbcb7",
    "c3c5c38e566e586e1ce236e42e1708d864b89c59cb475919acb1fd705d09865ddea4e633f1812adb9156c8b51369a695d321",
    "8fe506ab50534be40f269b2c686246402a4d3a502268308c7d895a824bf2857c9a2bd5d849357a518d3c756321d5d84b350e",
    "858d2417f085180db2b3403b8286c0ad1f36f2d48d62aab19b6a1c42356aa91b6be99a5c4a51a85a919252e44a29e2528a42",
    "d58a943c756321d5d84b3542454b00a588a014f1a1a2666877b971e878111a2142a344688c081d2e42c522344e844a44e828",
    "119a28429345a854848e11a132119a2142e522345b84e688d03c119a2f42216fa1082d12a1252274b2082d15a1d344689908",
    "9d294249115a2e422b456895f1780b601ba22423fb98b53fd6c8d960763d6901cc810528948a0b8e18a458a929a284bf2896",
    "80726348014703b08134b50547a63efa4923ff4573f4178d999aa6b3e048c58b9f344dbf682efda031016a39a9fc8223cb62",
    "7fd2fc03cda79f343f74a5423cf77fea8afe8507d1f94163fa530ec49379e727cd2f3c88cd4f9a9f72209eab0fac7fd0fcc2",
    "836481f2fa9f7619c34cad700c44b463f00fff44f731c91f88e8f4d90a405141ff8c8178f6aa7e50c4087f52209062d53f28",
    "209a779d3f29e43f295048d1f00f0a8825e5f10fbc314dbf2844700c431c140dc49b0691dcc9ff8137e61712461d1de2a5b0",
    "5254106f1a44336aaa9f9a2af6171a16adff07eef5a0ca9834228d4913d8b73a800f2c493ee4d3230da10c1d983b17a641ac",
    "f46f437ff043ac63a76bd2faf3112e649d5b0f59cda08b21a3f300b4b1009243e0d1d37f9243e0593cb6f307d853269e0380",
    "26a37c173b47bb6f8e360732e027f43c5e2034194498f9db24ede12b023915ad2e88024af581ed50f1aa4743b25d86cb175a",
    "723b2c510aa8b95b0fcacc028532a8669ab0d2727900d7b90f19d283d2d2a1ec959a540b0fb6b81f625064e950f4884b33b4",
    "46acf1e79db434f1e9827450acf4768dd6727f2de71ec4bd8929b540a1b0e273bf9abc3a507a0614f642cf312b79c4a0133a",
    "e87a30c79874956a682480c20c28f1affb7e051cf11a1fed43a670ae405749c6ba6f4335a0b332a0f02d633ccab4d70e0595",
    "399b699483e79878f520940abdb935bc353ec8799e999b278b096565422d82ef932ac46b8ad1433c04ba534fe2e43600b4a0",
    "1f33a19e57e3ecc0503649e60905723360682f848b9912ae9ded700d858b1a68079d68334a2d09225ac2252df5a299dec4f4",
    "1c30cfd704c250ed3bf3add1073436992d6786f3f97427b8782252c8db0e65d4c37b259457873042d4c797d43162089fc4ab",
    "ad0166a23485c3c7843023f9adaecc7ea692d9af81d15d992493cfec651632c5747b2e5c5cdbe0ae50071175225d89760045",
    "6822f5d14fa80e61024c9426708b35130c6060aa39a9a78423000a934101ae4c982a30f91a8406019f30662f5dc718aed0d4",
    "d6c00b579fa59680d1ea538b1cb875cac158f51968f025a599d202ee73c6840514af432f84982876be869849300a99b5501c",
    "4987cf143e0d57ba09062dad859f3a8853334a2db004e03102c608f5a166c815bc4fc8204d08730045e6f99993f1fefd1ab2",
    "7c1bd28f45ef67ca9832ba4d0452271b0a384d48877adfaa4198f11054bafa58560e4412e8266371200fe261010ba529412d",
    "372c6022a5d3a1f5e64afec312716280a62b0aa00f6528a0176a485d01dc0b2c62e06e5a0765d6208d7087d590809c1cb9f6",
    "38f5f16e480214640e719990e6b8932d93807e4c20d14e864c1bc4f0c033b8d1a5733f71ffe2d9f322793a3c6b3e672b2f98",
    "7b897b837b8ffb94a7c9e7287843f99cf53c133e5787cf85f5bff89c4f3c371ec2e76a7c57bd043621644e1333448c33c9c6",
    "08364ab6b3d47f181d7b6a2629248c005fa39dc59da552dd03989d2e3a1b73d502c85c402a016012284a36dab1d1c2eb6c1c",
    "c3ec4268a01718cb586c29031873a1302e8d04e92466674ac3dc5142accf24e474b2d314252234114c4827374770c966ae5c",
    "0fe1a3a842cc062956909f60692231b5fa67e04d175560ae70152111773ad928641012063b547dc8461d3815c2de9ba43efa",
    "0aed81c18d3969a634557bd9080e0c13a5b99243ade4d06bf6a0d4dace23286df95dcad65c2133a4d301ca0b3c4d63a3e5ee",
    "27698d3e9b680e737ce993c3229598e004adb6dbbf86063478902dc723a871f0058a8d4c9533c36a1d185882784ec9ace942",
    "8d76fdfc6224825b2e8e2b460869119d4eb673db4318f913687a983b0f93e48ca745384b6a4d9b31ec0c935b3ba04121cf57",
    "1f5a4e7a10ea1f41d37003391e109747f7e3444f344fc80cedc402ba99989064239c70905ebd058fd38303f49dcef8c06ab6",
    "b63384f3c423689bf53560114266c3ed28d1970e9d0ffdcc9a5f38937489d372a76106b3d95d9870138d6edb6b1860aec1d4",
    "01852c8fa0df1d9f254e44b3a9f009a38da7d9b13d0b697477eaef9326c0b55e87bc8bd914d0780ddeae62d380569a861728",
    "c5223c821ccfdd4ef443c60a99444e11be5307d3b5d770f7161b604ead348ec63068c924684b3abccb03b7b1352c20137457",
    "a1a27fac8f9c496c7be85b49c3528335f81f7d307ea2860602dab06a8fa0f7ae7de2005a8e9c195c8bc73d66d331ad6be6b3",
    "31872ef3219e1ccca9d39cc68ca08e26059d78905057c3041a0df11f096a17fbd248691f338c8ff1ac4de2b02102133885f1",
    "4a3b61840146d734b111b24d7c3f085826486e1f3374fd83769f24033bd73e2bb4a2d1ce17c5d03e2b4c2833a071a8bf1b8d",
    "17540a302d9609addd23e8e0a20ed9783455c80c1363bc7bcc286c482f13e18c009305e6e6f873a7f1602cc6f866e55c3c21",
    "39e59ed85323a78f19c2c7e1e0c9c1f3ed7dfa994363cc3d7b8c2b6f62363dc688661cc8124438a0eaf35981d3e08043c61a",
    "36d000e84aaf3b77244369a4ac8919de99383448693cb4c4c1b99186fafbfba703bcd45960e122c184356c4773cf80793493",
    "e222d0f5aed40781cb742d469b0f3b6c70879b60403fff9e730d4da074c05c16d2741c760cc1c014c1079d1c80095e0c1156",
    "fa063ca6314033ce713c60f45e1e88a43531c3664f6162ac6e63c407308db1517f7869ed2458c5eabf2a65c3fb30c11a6c6a",
    "e0726c68f10cf7674f4a03913eb82f60da27cc3d9ed292bd2385ee36035e98701647231ee0fc405a099c773920e80f9a45b1",
    "dfa22f77c57e486e13335450e328edc7780d26f80b7f046db4cbf0aea1d9740fc26c9c029cdb82039c38d4d0b06ef6081aa8",
    "eb95fad07284cc60b110af0bf2c338ae428b121f2cecb64013af1f8452a7dbd913b61580acc02e36e5390c8ef72d9f7ac494",
    "e784cc70f1a0165d3b9640370051ba0abdbd0b1d7db6119853b709839108e3b038908e219bd806252dbcf30fbf834b089b4c",
    "a29635986ee01d93e93e3031f34762045f27e3878a105e2888159496e03db036d9aeac082e094a2ab384db1d5982179608d7",
    "4dd6a1bbdaba9668aa736702e6c730e9b3cf2c09e322f8d869fa8854306732575035d91ff1b12f2c4159229888e1b5a58017",
    "0ff3b11130c90cd18579dd8f845587300450781d9549e31d93581894288312957467e1bea9fa4884dc0def9d423346e4343b",
    "53bc5f028c437b43ef84ea8a68e345b4c2900b21d744e82a11ba66918100d8042bc9f1422608594a98c18dcac466e724a06b",
    "06b830d57020ed9cda9deeeea069038b6065521f1310312493746539dd15bbf053b592f3e92cb823fa60cd4e776b243af10c",
    "325e3e8a0d98d3b4fd4ae7309aba1800f59dea0262b9c4e8bcd93059a6bb36c34cbdd2e6c9687a19cc39954825de9000e8e8",
    "6ed20e73603a3e72623bde5d7cc60d647189614feab91a9e3000a036d79d884b3b60db128e2d4c2153e8204ce39767988e72",
    "a8e3f93522000d77cc730c2a5ce606ec611c93d902f332fd603ba7e98712f1c1d3e98818974c079ac08e594e17d1ac223e58",
    "42e94a91dd62e0a301c07168502201ccb170ffb36ceb671e6cfcac9555cb7060c62344236d743020a6f3e1fe5f0b430a25bd",
    "080380bd4186e6e1bf858e157e1bac3b35247173368714f48e0274129799b24cab4206cf04263c628aee728b5ed04bc79b93",
    "d15e184410f8f624662d9e00ed4601c207185dc687ff014c3f521401f411ba6d7d2a0b31af86f177733d58a51003cd334121",
    "b53038821fa585a03e9101777eb861f3576c77447b05588a964cd39859ce5c1ffe6208b0d222c25a9a322fc36849b02d51b3",
    "0de9827eeb8201053b5ed01b8d73bc31906f933f9829411acb9b672dff623e41cba189ef1c3f132d0371ac63b727b2f26a59",
    "a82b0c8ffaf06a19c902b8fef2dd209a4f64f1bd3053b9cc95bdc16d9dce07e3fe7be00a6fd5a9b0b76222a97af9b64b5904",
    "6171707201adc89668a90702633d8d48623d20bb4c148a7b3934575d506a253c1489965a2d2283a66af4fe8ed815d29d8d00",
    "8dfc2aa76bd1c86f5bb76ea5058b8b1ae93060d3204a33b81a8d6c5648395af3a9bcbcaa4ea88fb316a2a216922fb291cd61",
    "d2d5a74ca1533031139d9247f29bf007323e93a89f843200bd9f9da98969d9c9341148dee75c0b6ecfd4cae4bcbce32ae320",
    "cf481d61b529c2e8a7137689ba121a4c936ae99d74a8955ef8e310ea8c9fbb9887963a903a9e7e9d4050a78728e9bdb41c9e",
    "568df57690ad43443eab751cb35039e0685e2802fa606790fe768df5d3ba266b2b7f9f3684d6b9adf87393b1b348f2c5fa69",
    "502300343a38dbe858c965b2af6f2c0c60ef0b9a2b398e6667d6c13d8f8bfc56fc71369e9d2423f14ba358a5283b0267cdc6",
    "24b36dba302fbeeb843120892843006628d67efdf6cbd74a7bd217b89243a83f909243e13d21128843eaf445127df515b4e0",
    "d17b20d3214ae396636c168fee366c86299fde2837e5d07abf3147f6865723e100b490e31db6150b3d27035d54f935aecd72",
    "cef391dd41e89e273dda217f5c7c562bdba313ffa2aae93bccdab87c9a4313571e8338f47c3791dc43148bc426a55ceeef17",
    "efd3f930682aa74e368d31081babc6e9b3e3e82bc56417a1ccb646b27210600820faa023175fbfd12f073df073073c01c760",
    "b90c5e80b348821c09668b47ea88c5720038d19b0601819e92cc85fbd1024028a913391dc001c170dde14a8196920308c0d7",
    "a54e5c861ca18e60f06051ad70d4fc9d136dfac7e351f17cba5fff78802675a47080199b0ca5ce67e8286becbe994178e0a2",
    "f026a1ffa29277781b1fed0427100da1d3223c66b846097461703c0143393c6f3cd0137a2e02ba8e42c713c2b4f54f642020",
    "84c8eaa303429f4567b4e4ad6aa2230449246fdeda424c99ad451b98c364d43201438cf10db7b6e4d51aa00892b76af356d0",
    "32a2cf8285d813b51cb4653de9236c760927dcac72ce4f3469c26c6b9129be30972a34011ad5b0ddf5aedbf13dc02f84c8d6",
    "09d3114e500dc85ce9327131975e48a5f732a089003e0345f26769dbead98c78067b84a9597926fef3e6ad0858e3c14fe422",
    "70f27cf3de02200ad96280a4bb4bbe374356597dccf220080a42975dc9a149ebc71baf6a6a8a411d967d620e133ccdc81014",
    "060176cb169996a63b67492dab6a347b1a18150aa6c2a8aa1606635b91c23d9a5f54aaef8c7197e0f8485957fcf5f7e2fefc",
    "7d3078f886106253a8c295000d67277f3d1d94a1cd982c0b62021af81e225ecadf2cdb09783d304d3c06fa403b1c6f85a1b5",
    "93bac2f4e1200d4aef9e0a868782c9787d1266f711295934e43322e3d759694cec8e0701ba44b89089b3f50010127aac4f41",
    "4cce4883c0f6992394426042d3a27d844a4b9eb200f019dc4bfdfc1461e13ef6000f06c381080e933878df085c59f833bc05",
    "3f62314d1358e98350188e6255b338a81eb00c5116f43157a84f9470fd25386ddea67a0b41f32c2452c8accd99890004cc16",
    "b35826d461530acca877f7cc61c997b3824b5878d3747a610acca58fde9ec36a5ace1a398745e2441d0f26d2fe4c163a53a4",
    "24f39a98755bf05e0326400a0338d7364e6cc0230a58c680a11c189cbcf0d4c44f53188766d6e0d34a008a7bea0e2e01ce17",
    "c0677496738955a3b0be58909309d2b8986cd49f66b39f8c1afa998675ba26576f7d1ffa66a6fbfe150df6b22756559f358c",
    "bbee04563d7054e41e5ba135e7d343dd7527d66efa3c738e90f4267d803b5c0e3c4957d21b46dba6c03a12e3356a016ea316",
    "c1c3b8188f84092ac92a8f406470772c55dfab9131683d928028e508b713fd18054a411b2807a7c03690b806a634f0fb3238",
    "0b5e8166f006a65c67e1ec6b031df02e0251700a9f0219f0a90c3e13c0e7054037438a53022743e40e9ce7857b197d906629",
    "1c0463b49201a6f10acc07e2130d3c4e1b68049bc15ca8a81e36f9d351f4141c27ddce5f51e411642c190cd00a8c1c04ec49",
    "214cdd8dc9484621514be6099acc0123ac2eac4e342be676186d361a02d2e1cc0c31941820b8b10eb31d1039d48f1c37cdf8",
    "0eb67e72c19556001defb8f8dcec2763f698cd7ff0a6779ad1efe1f7dbbe0ac2cddbb60f8a301756f2d9db8a3fd1895a4f06",
    "838049b2585f132e057c53ea87f0f6c132413556b200e90c22a2bd4548bd799dfea00e1d0466964c41870eaafd5aa5fa283c",
    "a4836c09229adce240b0d06d2408711f0944b044b88b61ce0ac531883c0006c7d0a6e2a7b1652368b5fa3ce7581a815bead3",
    "01b62a8686740aad639c53b6072700db2f7f084dca042703c5b8f16e140dbeaed4b53fb49726342843f8082ef1e1d2413307",
    "5470e5284bd6b8016f9401677380ac3f53f610af0e43c7f63183f9b865241bc59cb148fba618844976ea62165df3508e3bf5",
    "932dbe6ba406cceb3098886c7f7d43eecb686206773ac014dc7f13cd7da834cdb36ce1095fccba6ca1bdb43ec05fac610069",
    "61203db6e1a964080a57e3503e2e9d851977c40ff95adb3004afb1a57e4c1a496002c942162b44fde392b13062c7fd270625",
    "9630ad61180e33b083f305c289489e9c29aaf4f312a41104817764797f8d67c0049ecfe71b13438c2b309b39a33976671686",
    "0a7a56e1fd05417c09851387a99881be3eee13ad59066350a283d022b9849670587433620977fae930e60cc1df8ee0137ab8",
    "6c138dd02775003e8d75c9298f9521f7a8060c1f618ed3ddaea582513a644192ced2a11f6379b71c800001a3ef25035025be",
    "dcc8c02dd603bc752d8c0018aeb8643d5eb25e9b06e08461e9565781581eb0418844eb440ee0246279b81533800df4602039",
    "3aaf16ee3f2ae6185ba08780081b318b2d3ed648ef64f6c3d925662a7bf438dc467675d5d83b1c6080101159896c340ab7f2",
    "e5e4698dda3886263ee66587e6698dfc04ab0f12d934f8cae31e0fffe04366f609800f8dcf9dd2725e33d9666a8bc4865315",
    "5518834c0b03e43821b36eb6806e6fabb09f29b4741bb7ce3911081becc52759326044ebfc336f88c9e9273695c948e74c72",
    "a320cc1e9468d200aabf14dc14dcb5ed03970574212a87dd33c7bddd0e38455db2437ad15e9445334677d258b45ab810eca4",
    "45d0ec686d6823ad9d46d2ec604b2fcd15ae867cc48ed60813866af8d48fc076c4e715c068523660b1208b0c8da061d40b28",
    "8e44eb51639485ca50ff01c0a8a71943d132da574ba49e5688c82de18204021116f3775158b0284914064b4a996e3a48d527",
    "47c9994ea0a5652ba34512bc15e5c079c8147686089d6c5a11db9eedc01215b257ad6a69a915c19899d9cb14b383af0063c2",
    "92a04ee52c00977027ed0411822816cb91c5665b59efcd3619dde45c8708449e3b8d9a508cb91c215b4d8056337cf93df011",
    "98a14f4634319c38d43ecacc4eafa3fe510ccdd5f5fc38c39c6c1341a73730f7a0713ba95fb2702f9e6aa20ed0672b6e9c44",
    "9b982de1bdb651e76f768a58fb9e0e49911eeb0ea2d0caf12eef1ced7ec46521ba774e0823e70ca22163d6c6f7d027d205dd",
    "63004f88942174a40cae79bd70f1ec024a58ef85f513f0d306940534a1a06b27a311e6331da009b5f70905341fb2cd9a2508",
    "27dce7b3588227eef3d9567549d6825749190ef88b240d6b638405576f3acc31db642c316a8756b888687aa3d32669a60b2a",
    "93dda1419a4e229ab624703e9807935352e4100c931f6b1622413c11424017e0b3333978533a3b1c84830498468601098814",
    "b465b24374483e6908d3537da5be12ef4aa7f15bfb611c8deb886bf133371878cf75bcfb066d28f5675663dc5e88d8619978",
    "5fba1806efeddae5fd2254284271114d5f84da8a50fb018910d365063bd4f20d39af833694d25859f05957d3c8b4e28921ee",
    "1405f433e44c6f0609744840189243ab1e87138426e0115ca005f82457c983d19f16c1c1eb7a0042e8103c2587e0c1d73c52",
    "87e4c0045a4247798043e848b89a248f846c0457c9055c414e340b56c9b42e5de55194c4cbdcd8ce0dc68b2e4eafb9b59600",
    "c012cff103014a18b1a6a69add3091de3c97046f49b716488d136f7627dd885e6b96a2ad7dc72c8565a19036dc4cb9b538a2",
    "26ba86556d92527feb86698ae262c49a549305c359d5da17179bacee599d986a7e7171214cdfb4575958986b2d38d7d37dd3",
    "2425317a2199d6dd608cd4dfba152d36eb30e174d470b546706ea839b41793a8a6f9c53a089001e367b8f33823efac8f8cc5",
    "91e400bc34421ba233ecbc756b8d49ca1aa3ab965213e975b123a8b7cadc19da5843b35e7963dab08caa8ddc50da81b95f41",
    "c3c915d66650d9fb8c9d05b7ef397c9d1cb4f2f5ebdbe3df36dda9bbe2b1ed5eed3559c4ef470e8cf174d2357acc2f889f92",
    "46cccb1a153f774993941f6fb1d53bd773f6c72d01dabb971c9a8b1c77dbb19b6fe9e850796d77e702eb14da2adfb9df9236",
    "8f1c32e9f590b5a71fd48e4e7dbeeb1152d9bfdf51388bb176fe191df95293d947d8454169470d1f41045f7af49f7e2aa7bc",
    "dee6c53aeb1765fa19da610588c4b12830bed5a390ba830702393f65673b5d675caf6245b7ac3d33253167b9c5b201b45dcf",
    "c886c9015733af3fd6d2dffef9e69275aacda7d2a433b7acfeecfedc75cc1e657408cbe7a5c6f3ec0b4bfd65ab3a35cf261c",
    "fc78ee54bbfd8dd6fe5832ed4ce349cfcfad2f96bedf7ed3c8c3e1d093f723c3641b64019cd8d3c141c949cbf3184eedd7f1",
    "4bd3f21e2de38fbd9e5ef7a2fa4e9553e207c5b977bfb16cbff60b2ac23b7c4f15aa06498fb81d683f7db8e6cbb4e40fb18c",
    "b04fc14b4f4c9f35d8f1ea932773c509eb79b6facb1c2adeace85915589a3e891391b3cfcfea89b320e89e10da7e5b9e30f3",
    "d5f173987da07c6627deb0ab7dd39d2e9794ed45c3d4ee0843af12639b578e0b836b496233187d6b2d078e32bda20bc71b59",
    "749d927172dfb0e9d1818e3ac1a1f4a40aac7e66c6b67ccb5d4569e23da7bc4d4a3e87446606111e3e4bff683bd39ce475a5",
    "d099be212aa8a35e4be73c9976f6494c945364d41f97fedabe3d3dc8d4dfc5eab34df94a74d6fadd2fb7bc2225bd6b34effc",
    "c5dc654e10c00b288180db82d4afe57457e60050e826f992d2d8ceb00a2bdc77892848be25b75ffaeacc7bb4cbbe78b96b84",
    "adf691dfaaae4c7a9afc6ae5d77b7d0d5ff68f6d38317851ecf9699ba0bbbf443577bce56f682ad91e15b53269af4bf70bd7",
    "55ac57bc89eba2fa2fbb5f528cc19c4aed63276cc999626aff125cb5eddc9de3a7b53fc67dd5351317f78b57233a3f0f4bb0",
    "b07f6133332e6bbe8255a2af7967e83994efd9e79339b14c3a54e13a7bc6ebf98adfe7d71bae1967e316b16f7adb8df083bd",
    "77187f7596ed6e6ae38c8a86087a5529bb6e1fb4c8caad2bdc55c2717178b4e1d53ef681046ce448d3f01a46ab2f7626f9d5",
    "0acdfb275fada87bb34b62d5fac9fbe3aec89537c32ebdf8e030aeb0b7512e9ce91356ddd022bc388d256b6a1f192840a71c",
    "ad9089ddf5aa0e6c7870a27370d196fc9018ebb284679b5e2674d8a4afb1f2f43b7665ebb2cd5aab2b7e3bf81676f8fd415b",
    "0e8dd1b960da12d7c11a164dfb5031fc70ae71e86f09b107868427279eba36e2c6c7dec28b5d095f53cf4ccd5df8e78e93c2",
    "9932f720cb3e9f111b1674fd98f73632be560f9dbdd40aae4a836041e17431c9cf295d0026147b8634b633addb46f5cdfa60",
    "935ba83c3fce63e67d938898e3d0f6a7dd0f5f9faa9aee8bdd7b70f5f6a077392faf449fec3d6ade424c2ec385c7eedf132a",
    "de82d4b8ad4cff0fc7254c83b31f13f58d97e6a7685e49addaf0f6d59d398fe77d5be9f4a8e2a4e7c3b97ab611e335cebb3e",
    "b72b70c9de7e6bf5d1f9f3f256de9cd2fe7c709dae035d4ffb7ca57bcd593872e824093759b8a02af31d807ee7227e6c8c16",
    "ff3c2796db3172412c6f644db4b8b633363a3666218fbb70c91271cc259d05dde41a000402c9a85b70b20f46dbe65f4d0f0d",
    "5d62b61e97ffae5d1bfd11e84a12b7591a68a535a76e0d5870f19ced96ec61a3066f7f70feab635a8cc79a269841adce95fc",
    "b572d1669fb905223f24e0ed1fd0f08be7513fae6fc59cb9e8ba43d1cb42ce0cca9cfcc746c3addf4045f534149b3034ee68",
    "afcab3ad8b1695b464bb416bbb63ffa584a7ee078be9177506297b2de3b472863b5bdedbdb7e29556fcc12c3f8019ff126f7",
    "7959bccdaafad7770fde19117234fe4d9e4eca7241fddb25876f5cc958e16feab6e4aeef0c91b3ac79e74c9a48063bbdc7ff",
    "f234c6b67349e35e0d49dceddb79e4cc65b8ca6e1208a6ddd830fc8d728eb5574f67cfd0920333caedbef47d3d916599f764",
    "68c84917d7cdf4ecdab76e41417e2e7619ddf73df67fb93e3afe51dc7b339727b23d37c4af66392e78f7625919a11e168358",
    "26ec53f7e6f4c59304e02a95808b7727d0ead742ef9fc39808732d4ed3f592f70b120d0108b13a2a1d97e2f30e2f29a73f4b",
    "bf5b1bfa3a71af268fc9713528fb765dd4b4bfa9f6c1eb08d3db9b0d47244e38326cc14c59a520f5624af2a5f2e1430ba433",
    "1fef5e95f03c6a7248d50ebddbe7a16d97b768d8e55ecb6d3cd8fbf869fbaac9ee9323973b4a27b6cfde6055515ebdeb70e4",
    "b6d14b3394a50e7347862303cc852ee165c94795099dd2e1c254e0ffe7e1da59fe68d49229fb19aed91f5863b4970e3cce10",
    "299232b533a79176cf878e1d6cbdd6d139de69f5819bef7a1fa7355d1edce038166b1f3fc1fe7baedd6a03d6a50f6dd47eed",
    "6f0b5ae73f5d63db882e8a9f34df5664de707ee5626cf75f5b13125e233b836f3b3ffbf47ce70bff2bfe263af36973c49aad",
    "061ca6e5b3c9bfd76ff8f672ef8ba1b92b2e6e9b915e3345e1f211fbf672a5eb5cd4aa7ad7f6a39cba92b2791b4f7f8cbad8",
    "bc4cecb5784b104927b3bcca1f8ffa6bdda7d6c743ce7f2c3fccb8afd1a50b5753e551bb7e23f6eb2fe716dd0c2680361d00",
    "de8c5c0e46affdf503c2f45bddab1489b7cdcfc1e5db31e1baf4d1054650d4f1c0e3b6de7a4da7c5f35e363dd8daf7b2eeca",
    "85ef7dcc4ca56d3ec8908ff43de8f8a1fd04d3091a387067b1ca5828f4dbff6ae4d5fe9977e5a3ce5d7517389d7a7ffbcccb",
    "25a38a850fec636f6f4dddbd296542a8b27ffcb405f9e74696afadff7c73f871fd9d7903cb3453759ead14241d3c79f71a77",
    "eb46efe55be867536e31470cc8b6dbeb2fb62bdb1b9fc15abd7defabd77bcf69e0cb82a3c65eaa5dda3e5fff05c3ffb8fe6f",
    "6ef3ad2e0c8208de1bcdb50c5c7bcad7eac5cdc37fceffed9be2997677576558d5d9f0fbd933b7bd9b327196f7e6e21b7537",
    "6eaf2d1d7efffb5f4587a65c897cf3e653d9e13f0fceddb055d464c2a1a79adc3b7d82d9375136e9aeef02defa2d1df3a76e",
    "f41c1f50757acfc2b9f743a2a3e2fadc1ee8aef2bd33223c74ddb0ec0ffb9f0d9bfb70d6b4b2d7be073fa37012df5c9df2f8",
    "6edc9d199ad785439ff0973ad44d9ae876c1bbcdac6783e11f96293b1425f337d85557bfca4aea8bf0d913ba2d81734cd61b",
    "2e886f168d5b9ccf57b53bde3a7bb9eddd7beda4a9e376351c8f1ffe5c382178568bfcf7a4c9a3af0ef29abdb9cfe16cfde7",
    "5d8bec8d96de5cffae37e1cabdc6b4659cb76e679abc16cc3b2ba958083bf9d1f3f64b0f5ffdb967fe0595a5c697d85755cb",
    "ce9e3b1afd4cc25cdc68b0f955404bce0daf40c58df8d5bddcde471abd87e6c867ddf35f9c8dedc053b4dfbf7dc0981722bc",
    "70ae7cbfdb9649da3707762c1a32f96bc87733e7c0b57ab15f9f9f9c7d5df145c81db2abaadf5ed2f36dcad7f30f0c873d9b",
    "30c3ba30e4d846fc59c8d4c8798ba00f9e0d6d4fbc8c235bb3a457b0a2db03532af674ebd668dd6e1ae1071afea4afe57c17",
    "1cb9b470afc3f27d773ae659b839dfe8d0ee5e917534c552794ee7a261d4f9dd3bc2cabecfeaaa79f9ecedc42b91a9f3ec2b",
    "eead305863f322de67ba36e67c6b8045271e3c3e30edf8b9ca45965bacf5461c4b97448a5286494fcf74d68e0ef0b183e3e0",
    "4e0ecb6dc726d3b7e5779b4b23e41dc5d2fef98ed7164c89c9759adc94e92beab28e17db9f6c3f897e6f3a105fbfcdb89eb1",
    "fbf00a13fbd015a99f2d46f84d682b7af92db2d7b362cef3634f0266e6b317b55a159b9ffbbd75dacb7af1dc8fae8fabc7ef",
    "38fe3125e2c0aa28b195606a56c332dcf972c1deaba21d96ed1bf6bd7b73772b5c6e2e542f4a5b15f2cc6bb2d7509a9fc0bb",
    "ba7fa7f3bb8cf45aba86df20b398d33e67ddfbde1c7ef5c7310567c6f19992842cc3ae43788bb47fa1e9ace471d3ef05bc3a",
    "15287b16e81ebaf05bc9359953e5a32afd85626c44e3b2c33c5d8fbbe6bf677b6c7af7f6e09429c36f9cdff1dae2e49a2f37",
    "76af5ad7307cc6d3becfd3cbb66d7be63c0ffae0b877e5b395bdf73597fd6147df1c52f6f25b4e8a5f1916e59b968cee5afa",
    "e8d3cb97192f17f93b214abff3bf6de8d87a34296355a0f6011b15bb33a143ee59ec78acafb5e6a955d036e664996dccb775",
    "25b923d73d08d8bc3777dc7a79e71efdab23052681c9763d5338a392f799d5be1f117e69f5bd3b0a9f0d2396fb2fd6edd4c2",
    "b3e038e85a59874c5e24fb32521c29f2fcf2a6b2e7ce05e36f97078e9faddc5fe03c15cfd12c9e27a18d37c80fdfbde2f593",
    "f492eb930499856fd74ddc7276a47767cbea5bfeb66feae7ef0b717b726b9d699b0cb86c13cc06b185ad558352f39f85df7e",
    "75db8caf7d0164ce5abd6bf814f9a1e4cec14854dc636ff2da6f2931c7dad67db2348573e1c19230a3e6cb6b2654692e52dc",
    "5c98e7e8365f7abe13fd9a62e0d735fae2ed37b73f9f97dd1f6411ff7d7e08b325b3bafab7e947272fae1f38386dc3bdab3b",
    "0dd7bddcb543b7a9e8eb99e13272b74d74831deac0e52e7d427a4d9dfee0fa51b7eb054e7e359d3ee64519bd351daf78a9d7",
    "92bfdeb3bd59566ed81c78b83afacc8e23d72c2002d5d4ea6d1b33ec394ba772cfec6d6c3dbdd109a9d5a8ca3d337becee9d",
    "c9816314d944f7d1e6d864c1a3d6fd7357378f09f090645dde72cefdf1f9232b778419d34dbd0b5f2b253ff32800a8444ff9",
    "2fbfafe6c1b0ca0446bcff69d0027ba1e35fe296db0ae91ad3c41bd273f5ab52fe999d5152bf36f8c71c257ffc8a9b47fe17",
    "52ff3514ba6dbe2a314571cb442db3fe46bd3100faff8a979a0b7f83acfc87700cedb465a65a400130e5944a8c9c7cfc38c7",
    "bccb07e4d43f578581198f147639782c36836e7471b563de3095c2bdc9ac672ddee88358677686081a7dc0d4f66db7ba3ee7",
    "969f2ffab2fd002d526f89ee20e3850181bc914b9782eba1658f821edfff7371e86ab71aadb2cac3814485d5c8bfd646b54f",
    "9cb506bc9b5f99e1b3bfc2d6abfdf20a2edb98789a702978ede3f4ed766f1f5bf5b90e9b0a811ffd7e92ff70cfcb164e1467",
    "f39ca8d630afe9e75c4c0e1ee08ef76bb65cfb32cfebdddcf9cf0edc7d37e358c4accb6984be23d78b7de1d2ec432ae83f3b",
    "40002d9b32178f269a4b3d8d0db3c35bb7a5f5d0f3d018db2e2fe8038144f75663077d70c8e6d549f9582c26b0bb6aaff919",
    "89e6b627346e9ce530e6886dad7dea9eb7c5be56b17a0f1ebfdebfe4db86e25537efd46ba5870af29bcfd71def18ad4bbc8f",
    "939adc9aa9297bebce1c73cd586465be70c1a2a78e6832bf76e33069f0d319a758fa0f6e07f87885263d2a3a25dcb9efeb25",
    "6a7ddefe986b7227a7e94a83c675b4c92cfe5d796173c140d48cc1de86ae89ef7c9eb7267e6f1977aa2a23609d43e6ea58b9",
    "690bbb6ec78b473bbc8ae5a3ad6b4ebd371d78a928ce78176de9e97fe5d27e0952c5cfb2715ecf15ef3a7e75cd374faff493",
    "efd7d8f8d76f9ad531316996fdd9e4ca670d1b5fbd1f81da342e460b335e8e7b09d7a6c78adc1da9f35f9370641373004a52",
    "c7da7f3bc48085f98f51634b8e183a6998be4cd7aed7c6a526ac9ecf02c089a4c669dab5cec19d612e5cc485083dc2c794c4",
    "a0d250972ee07228a89587da0012180b078298fa90f032a5a8d5c9850c3ccc630261a957ed229713884bb7cf5e2da7a3531f",
    "74575a7446d2a810d87891937048ab16f57f870926301df110971cd4a5c17d97b16730ee35428957fb617416cf9d4383705d",
    "48e75dc666216581be2508029574d84a7ded4f38b7ba956cb27db178a4114fd0942c41d3009b46629ab574facfe967cb3290",
    "cfa2e88fda0c04317ecd3cbca11e3737901acd0496c7314e1d42fb31b1063519e69920e56e425aa997602070bc895728c656",
    "20dd2e22e31a136175bc4b8c71be5106c2ae28dc7d2e0b0e2e9f36c2b1d2786776a9890b612837ce4068951832a031282204",
    "ed594846deb0ad34dc7be368300eea356824c40527bc89483293c9d747f9dc61d5dd7d829d29487a1fd36bf0ce149a16d082",
    "131667a5d078049fd022709f141a17e8c00fbe3305854913d0c1819446f2943cc04d400909077fb899a7bbcd8049fde09bfa",
    "d5b6120333a0a0d78c4501d341e0ce14f6cd8f7b5d97984f7e6cc7e8e46bb8ed4c01fbd20e0f5a7dc0fd1c750a722e84e5f3",
    "d0ebddc611b9966e6b3b91feb02fd42988e699817eea2ca0f1a3cfc7d6d7ba453f4e41deafb6ab701dc9f7ddb4f974e3f6cd",
    "ba61357f4c47bdba371d98bfa46bc0e4e1c72c479d8ded57f7df9db1da83685c2e7e17eae8b660f87469d0c48ddb8392030e",
    "5b5a2aeef138c32657455fbde66f6affe96e41383333b84e5452bfb4266fc4aa29a2497ba65b4fd7ea5ceb32f948a7e9a423",
    "3f4e41ca0a26ef5b9708823d323f20bd9b6e9edc3e057bb3a6387ed68f256f902cca65c0cce6901966d50a3d46a65d3afb04",
    "d36c4d41cfa71f79d980a1ad294c71eead14b9fdbe49f3f03d291ef67b5246c1e28cb7a600048a7f383daab9e7b47518eb69",
    "6e54d4caa97b05b01d085c1fc54e5c17557819bfb47f0cc64d7a1a3b614b447281fe7b326f594e529dc520e95cf7f0398ba6",
    "efb7beb9843d18bb7f708e73ed96250e85f311e68145be8f11229959b3833be2f4ee8d7a5a261f96ff556c303bdee1c1a6fc",
    "cdf65fa3571e6c9de7ee6b5519c9f91cbb6bc82608e159b7cbdd2b51a3935aa3577470179a7f6ad9dfbc6cc5bcb1ec1ac1d9",
    "62d97c745590c6bce447bf357dc3c7edd9c4add934e47c60c15a27f784f78b27aa4f1e2e82ea88afc5d12387ea6bde69e55f",
    "6075def7678a5e5784938df241c3bd18a7cff45ace1bfbf3e461ceaf93078f63977e9d3cc05e7ebb41e3f1a1990f1abcf89b",
    "baf98b0c3576306f3b2d2d3c6dd872cbd3a563e08f85cdeb76bb2d387ee492d1c4290f23f552a65b9d8d38a494cf9cd1ee7a",
    "77eaa09f270fe5c27e43b4b3da2ee82ee574603e54bc4b79bec44fbb7e45912e34b2fff3e4a1e32ecdbef0bdcda7f4e494a4",
    "6b03939f663de82c3c32927429c385c79d218b9077e6631275b0309d7125a56ad7a38aeb69638c5679f74a8aa3cc56d475bf",
    "59757fd1b4d0d83db44ddbacff1c55fb746440d4e931f7f684e1b513ede4b6e8b927aa7df36df7a4c4c25922e8b002ba9762",
    "2f2d8ce52f8ce5c42eecec5810abc55dc819ced5e19f139fd719b9248617bb7024bfd64bdddbd18bccd6e3595ec7c6906197",
    "f1cac6bf72de1c0a7919f132ed7b62c9740f3f57ff836913b0942fb29e82f54f5d2b4dd98f63fe6cdd79d2e319d6cd7e6571",
    "0edaa66a16275a77cd78e1aaef8fdfbe39aada37c58ba7d8a3b535ac07e58f3c369433facedca136a7d7df5cca8de9ff3d7b",
    "f6da61d1dbab97a7281eb7b16656010ffe9bf289d2b4c4356e674df77c3fda3aae39cff5dec5c9ad1f3f6718161ddf7e7adc",
    "8d27a5ecf08222415143b17772d0735d51c8e9dd3371ceeb61cdcbeb78ccc1ed9d10c109b3c5e543129d9ba733524f3db01c",
    "3d2de892a964d9fda66d65e34695eede4622a533a6d08d5c66588eb9e739e3cdcc10f71d0625feb343561a17335b0fad9db7",
    "7fff8dbf1e2ed8583d7ccbebdbb767151bb92af63e2c5fb9e9f8d4b2edaf57ba9fff23b4eb6f618bfa28c1114e31a8f80e07",
    "630226e60d6b64daf9a55bb0941fed1ff531696b8a80ef612f9b4e3b9b1411567fd4a93585ca7aed5f383b525ea7ae625b9e",
    "7b6873e2a3333e9f730f4ef27cbb58e156dd7bc51b18cf130e3f3263cc5140c2c1f972c5977ba78fd35b8f8c08f8ecbeeef5",
    "9b8e8f0fa54df96d7bd6f8652497b0ea1b46cd73be31afe0ddefc91368773d36fb71621ec8f607782df7bcd1a6f9d82077ec",
    "860359cb6d87794cd9ad3aafa383de9c5b3772bd0b6fe40c3df3eea6cbdb36e3732e9c69daeb9eaa78757b5b0f9bd7966ba5",
    "511e1d927b2fdd3e9bd3ee962c305f0f11f4df9f163cb8f9f5ecd787f73c7bbaf5ea8582119daae48848760e7923a243f2ee",
    "d897730b563fd77dfbe8dabeedc7de3ddaf7b6f3cdebec65730e9edcf0e8fd94d030f44d44dca8fe03932a342a998f97ec68",
    "8529aed7faaa8eb3894bc439efd7dd31f47eae39f1777b9d4c6dfbe57cc3b79fdf487d1df606682f7b37cae18bdee34f57d3",
    "2e410457ba4cef7cff62f866e1bafe6069e8fced76e8f2f83467bbe3b3e61d7e6616eefecdf92a534b9e657f86fb628debb1",
    "b40ee5398338e5ba769d66f9d97bece36f5d87147d7e7fbff5302f75e3b5f1e3f297f14b8e9b2a1c7bd3ce2e491935b3f16b",
    "dbfb7a7acff2dcf4095e15bb0afa36efcc9cfe4ed83bee74eb31557e48ae53711cb6092eabfd6fa71e78e3999eb7232919a9",
    "fab2f0d5c3bc3f3ebe59fa76e9aeadb28a275e91bfadcb9b6f5a307d45b6e4d4470fd6d0c34facf853b59fac0b785a6d7671",
    "c898e1576ef68d6518efba23d8e72dfcaeb8e23de7d99d0b77c1d8070955cf0f241dd993b8ef73a686e88f965960347be124",
    "d5c5b4fe4701f2c94baf7f1cfc642773d0e73ac76a38a4fa1ac754b52acb25393ceefaf9d2c0d46d1dbd67b2e9bb052b23d1",
    "578d1a83033de6bf6e7bdd175ad0b267c6e3d9a2456d7b3b1e69bc18e7aefc7d0be1ae8c7fb7e177e9d3ee758727bcf3e018",
    "598d3248d36d3fdc8247e67abfd4d52cdebd784ce0d10a514d99aff584754917ad9bef5d1e1520ef33bd4ab426d424f4c6bf",
    "dfd9590163b86f9b671fab3ea8fbc86771d0c9535575274aa259479312f29e0ebf37c760515f6b1ad1b20d9d1660bcde78d9",
    "caae7771a106151eae2b867d2d30604d3bbe2373eb17d1eb754669b3d91e89417fcebe33edc427c5b26f278a4e19ddfab0a7",
    "42f488d4f856e1392b25c4ecb464e465a274d539e787769daf05ad5f0a2adb864474cd725e1940053bef4445a571d73a1d27",
    "6e9db7389f7b73630f6dcad579a7c8650d4e4edd5b4ae7ea5c1bb67cca9e8b09d5a6db233f993ddb61247c6c903e42f38af0",
    "dd96778797ad1d7cffc494fb998376271a8c783e3c4aebe9a18c7487eacfb85eef3723a3ebf7b21ebe3875f6b1b5ddd4f153",
    "b6c44ef8bad2f4f69ce8da058f8d8ab5d3adae81e92193c6c191d839927ee6c9caaf09d6dfa7f5ac7ca938a46b6faeb1b9b1",
    "a169ffe1d4e49c7333aaa68342970745af3d4cf5ca2f875969bc8bfbb63ec7e194bbe9d04d17e2a631258b178fd359583f74",
    "a3d5d8ab7925d39fbc1eb3a639b92d8f1e387a6876ef56ef64bbdbfb381b2eb97c48dfb6f6fcc621157be2dc4edccd33360c",
    "6fec31ae7e126d7fe277e8839ea37bbf3e46abedf73d9cf8fdcf06b7837b9e563d34110e4c6a52f46e5a44efe4c5d9dfdf6f",
    "3ee4302d7aac6ff3ea65c32cfe2c57ad6165cfae5b97dcbbef06b32e31a547dbe18fad272bcaedd0cf97e9769a97e4cdf9b7",
    "16ae29bdb5e97ec534e4cc13af7de786e75e1b585791c13b73234372a7d3e165caa8da1982d3666f6332dc7ca10f9e366d89",
    "fe7ebf78f3a8658463e3f00fc4b989f8d51de2bbb3a670263ddf739cd5b067f1b4dc76537bc3a643f683e73cb333edbbd196",
    "74f4b9cc6996898d615684cda7cb2f1621d9ecaba57cbac42e296b0968d9f0e1f1c5867bdb6f9aedb3ef75f5ecdd3f3cea9b",
    "b1fdc1177f1c981b3bf01d2de07bcd105fd7330a7836fdf09f239306c391d82edb3b8fb1e091297a656d8ceb5cd2523ac8f7",
    "c0a39c9e1ac31d15b4b4444bba61e3b76676cab8b6e00d4b3e0c33c89be19e7fc4b4f4cb3bdd5e62e3a6c53399bfd6732018",
    "b010d613828400800f84e2ede101a38de0322efc28706e0fb5df66ecd21a8aa112a45adfa12c94fe33cfb4a91b6557136a17",
    "170a78368da302dad142a3c192d0134e2e31a1fe2750fc84bb9db19da37e2816dc90a2399bcae5e0d47d07d3394cb72185d6",
    "7dc77fd92ca55303dc093ed2edd6a474e37555f80a5bb314c78614f0168daf556858172c124777dbc625ce9bec3ee8d91fcf",
    "8f2598aef9502c18b2e4eb3737ffe4bb8c419fb19e02f26add7c4fd385a72cbf44b885bc17f764b92e3a79f74d8e93e91f34",
    "c55c7b17cb4d4fe794ec5ef23b37e75dc5786cfe6c81beafcec545e581eb9bcf2ecf58219abe3f37c068743ef4e30d93dc6d",
    "5736545a85d671f43e9597d4f7b8271fb75bb1822da9d9f9ac46ac5a153d7b719cd19fdfc6ed3f35d07ae8e14247958ef79f",
    "1fc66ef158fdd87365d1f735f39fed8dd689f5b972e9a064e430bed7262a233b70fc5d155bfb7c90f5f48a87f8aa4d473a72",
    "9367da9f0d0f3fff2d7ccd8b603907b923aa5b6e7efdce88a2b796702c7d59be8eecfad78cccad7743e9f53a04b36b4cc173",
    "d65321ea6da792f52ec6b12eaea1abac01c00cd7a710d4cbf624c9033b1f2f97fcc0bd664c42982fce5cbcd62cc5a5017539",
    "e1b347cb192787dbb5d6b9f402bc3bcb813f04178ed8a06d385bd8085d5fc9f1ce4187ae4f992d9024e0a58b3066252250ba",
    "bb68d2ce3ae73bff361898950b9be3834a103a1c0b30485a60bfd379a789dd13fd5a43bcd9681d04c9b2612507d5221263e7",
    "2ea341f2443a9c952fa9d4ec89d15e138b8afc288c9135e4b683b5719d5421f0cc8269941ddd6cf406003c6dbac6607457da",
    "e008e3660fdc69878bd8f8b0bb6668e16ee75a752e77c1a38970ac868994709bc8456478c0041561cc010d71b6a0cc108925",
    "237a70bba2b65d4eeb538ca103006f88aac01038ea2949511333844612044d49107c43962e9f55e55a60a0f71e11c9db994c",
    "9864b1aaf6e73e348ce36534d99648358121e93c0a3125094da085d7efc365fbf0855202174a693a249fd021858e6d30f112",
    "9c94f2f1ee7dba6e3402df2bd52170ac8d0973b3ff51dcb9c743b5f50d7cad3d7b6698190cc6add036e49223f748c58ce822",
    "695c2a6a84501125e4527286541225b7a824a72252eaa1d04553214545a99352e9a6d2bd74724a79d7ded439e77ddee73ce7",
    "f9e33dcfde9fb5f79abdf7acbd2ebfbdd6faedb5f6f787f42fae84cbef12f36596f373a61020c6b0167b976cf0e367eb6831",
    "50dadd8a2b2ebd59159e7e8c2b23637b91389564eaafe1f656a9020f2f3f5f7921d5ebd0a75b7a86b5a0c6b7bef4c427a5e7",
    "1f378f93be77941919e2f5367bcb0a4d33f736d9f7b9756f363535b33a6b2290805c9074f67daaadc8ea3bb664a4c798428f",
    "134f4faa25b54d2c2dd950696a97e723535c37d1e4369eb0716369f4ecc7fd0c5d5155737b995f7cfe29acd2f8c89d34f394",
    "b59deee29593a1c9976511713d7ed693bc4c33da2306d6c66fd2b7f79dfb3c2fc9d89f7edf449f97cdcfbe51186fa51270a9",
    "eb04a69ee8735afad6fd47a70748fede16571cb3b9e7bd61b372e015583a7e4e63f92c22ab30f2e18f524a032396478b69b7",
    "1cb0ae9d927126b53774bd920108e2d7c59eddc1dc3d9db327b5ba743fbdcbccd0455de6e6d3f3a6ef9aafe63c9b9d983beb",
    "b0feb135517b38313fc724916d6bdf2993d9eb2717f1f7ced699b1fdba4fccb8a0976fb79bf744c714a521356c76c12e1d8e",
    "cfe3e8a5794fb969171f0b67db796dda6fa77dcbd5657d9ec658f69502d63ddb192a93345e37bc9d33db6aed3d43c3c08a8f",
    "eb5388f1a9714d037d0311ffd0d5fc759bda8c1d6a1107ed7395baef066d90dbb5806faac097b45cdbf9694750118ac1a70d",
    "6b4eb13d128ecb4df66a3f5250b5c366ee5167f7e4b4abefb957337e4ed1961b45ebdf34777edc07c79fe75ff4f1619bdeea",
    "da96e8a3d7fb383062ebea1aebfcf3bbf5f889e3d7a55694d3a59f6416ebb9badb2b63b167cef98b2c5436e8f73e6c6d235a",
    "8ce5d25d8d7c336ea5e806b58e4e2cd4ae466a581da9864d3eb08154c3920d86d5b05c6f6e9e2ab374aa6f585e4443dec687",
    "337fc13a8e1d34dc15758454c36c56283c56b1532c19ecd83a5ad2fc8fcf6d2beadf2948bd94c6a41fa59deb8a0d2f9210d0",
    "ac16041a79865fe990b97fe8e10d3a2ac6d6ea2e79dffad0be08544fd86bab24f608ee2e1b9c9836af56f6cc1173f7a62203",
    "b9d15a2198919c4bd94b871a7ff35185eb0b93f63f2bb5d8596d6b5e59ac15dd6f3bd0369a65b7fd422f573ba4fba745fb4f",
    "3076d57eb664ffa26ad1325b317779dfc15dbf8c025ea3a5d67a112360ff4c137b25173a2ed175f99123d0731a017897e5dc",
    "e438196d4821939773df349323c7d9e42627729b216a75e5b4cedc2cefeeeeea3ee326bfd06c173fd3ccd8aa1600853259b3",
    "adbce0c5fbe9cd3a652cce1c85311b4c3be43bea1f9f6fac8aaaa21d1867f3b6e9ab5cea9c1377b3186b09b1ff870fdb1694",
    "5835891d27e5906f1227f49d6f4a52de1352dc1890b38da8ea93ae8ddd18d791b97865a76476a06ca6ab8d455940572267ff",
    "ad5ce562a7a5ef4374ca5f95e86418cd5dc03b4257927068bdd31f1b68c6544e75312c1d358dbf3de0b2fc6d2bb737f35ed4",
    "c6f43f9aaae876f659e4b8555be7172ef47effa158e35a681ebfc0bc7d5066cae8409d8ac377f9a840cf7297d95fd0d2f925",
    "b022f7bd5613ff65f5e9070965068d1aabfa0ee5f5575524cb3aad8ed5e3d6df1bbb25a2c4fe57aeb43036ada456e851a36e",
    "89d93e2d5d189fb8422c6a7cdb338a65f3ea5569f2be17e37c339fcdb3bb9f553f776eafefa281a48e177e635f1a69f0b3ae",
    "ee427ad8005db7da883035ab85e8d67708df3831863c1f6b0971bcf892dee15b86dd96f086eb0719fe2ab17c83a1592da58d",
    "3590843d70c878f68ddb4f2fd31dd21e8c1266eebd56d2e8132c9bed1ce896736da5d5ac5cfe5af9d16c6f0dfd6b872aef6d",
    "e96e460f4b9b5ef6a9bdc5dd9609cbfcea6e9e1868c97a38f3ddd68893ebfb5eba3fcb4c78a4e831b5a6c72856f5e8b39331",
    "6b2eabbfc8ec5cbf3972956e687876ec83cb2e675f3b47cf70f1ba1ca3167b3a7fa1def8a37266fb304def0727757c8ec896",
    "f9e0ce4bef6c95fd21ee7354e18945159697eacab419ae6c47a3570146c72cdfac081d2dae47e97af76855a2c1fed7f54fea",
    "8f5eb7edbbae5ce0f9f3bdbdc957134e72c71196310633266cda54f6a1ce8773706369cb36bb3bbb96bdb0193330cf61fbac",
    "d2f727ca52d8b2f0c27347d4cd7b7dfff65d7c57f6e9d0957358f7b28d82c6bb4d76f97ad33b24ecd7f556fe752971253c30",
    "6f6a68fb6048e83dabafb354b6b7841dbab73837d609c5e0bcfb9ed55561bd171d078bb1b80d2a9a5ce7d0eda76f716f0dea",
    "aed996dca4f425bc9dc1080a96bd2017bdd156c7b4e74a167de1ea2f312c2dcd62b186ffaa6577ef6d9b1b34a5f2ed89f419",
    "e19d8bd2b6eb1715afa625283f0b95531adc7f7aac9c9d8eea688f19075e8a122a97186df9b27844f5970feaf70c677dd893",
    "6192b6d0498c24bc47d5effc14f9dc57e1374a1cf67586a9f6de536057da1a1baab1ea816a769c7ed9c0de75c79fac73f315",
    "989f1a655af8aef0ae435a5fe1f3b3b547f67ef491ebf3da28fb2a86539a3af354b9fdfa25bf1e9957736e5b87e599a33a97",
    "264455369f2a5539e677d88d3eab5be97e7c7de5dbe79f5fb4e46cee4f783df302c3f6e6e7c8420f727c7d9f41f105c5363a",
    "4b3758ddf1a51fe3b3f2a8fb1e3f45eec9b93069cefdf3a6da96eb9152f6eb9e79b90b575fd13ca3c51bf9bed761a246b073",
    "f504a12aed7881ec1cc69d77758f07afbeafa21f6ff9f468c7550fe61df539d123833636548acf7d3c9059299ac4d62e5b7d",
    "a529f0a69dba9375cd340dd57d84c9e3a8c771fdcff242510cdecedd99ddc6d76cf5ba14f5f57e804ebb8e9cb287675e8cc7",
    "88984766dbf33a2719106c63a65be57a437dab97edca4e5b1696e61d9f34cdf36c5dd8c1e3e1be315bf223fb3ea5b56189fb",
    "e0741ba73389b736bcd837d133c1b4dcf1a9e36a5b0fe64785afc7add7163ad3a5a6bb77e67485eb5cea08ab099614ac7980",
    "df7b3ebe4644767d7ce3271a452e6ecf2f0d4a69d478c6ac9d5219e869099e181abfb8a7f794df7cf1e1d913ef3624a7d063",
    "1b0689d6b44b1bd53b67547545f7fe10d2b7bc207683f711e5e6c38d7d3f5fbff6a9719ddbd3ea59b90a4e636885e646db76",
    "191d7cf7ae6f65ece2f0c5eca5cf2de6cb1ff4a91d8828a9b199bbec68dfbbd57ec4a6a3d7ba8d3224bfc716654ea768e855",
    "256f584cd02a1f24dc211b249c815ccbaa210e5051cb872f77ae948d424fbdad5f9baf5783cd56ee17a5d2ed72579eafc99b",
    "f7756d3b56f7ba6ae385e7a5d72e3675bc88efa74db499ba4dc3dd23bd27eb69b5fd53b5ea1367b8ed81edaf97bfcb52b6ec",
    "9f78c26f4b3470eb7ca27d5ddfbde85cd5f19a6591c54bae8d57ea11ba79441f16cd5fa616ffc3cc435d3e89cfb1bb15d759",
    "9f3d1f1afa97790468a734b9fa4c4115c9e350f98246ed273982155a3a03596ae7ab363bccded3fd2889357fa6f6b4457c9e",
    "7436b85de062fff5ed8e1e5b6b5ccbfbe9d15f967eb57a308f913c297061c2c6bd8fb0cd6d5ad735afbd9d8a9be1d6473309",
    "833acd8587976d396a7cace35dc645855b846a5156575585d18455476303e97051e181dedc43db0571b35a2b792b9a51f135",
    "1c68739973e3cbfae7aec153982f85750769d770e31ba768f56bc2e62cdd16bd4641f368c62a5183b51adb63819a9ae363ab",
    "971e156f97745f8d501de5fae0a02e1e397079ce625065e4410e565a2960cd929589abdf3f7c62eb52b23ef20cae6d50d1a2",
    "fe6a6ed524959579a6fdeaebb72c69e1692536d09b3fdc5b5a5c5434195524bf081579faedb6eba65c8b48733c93079a69af",
    "ca9797bab92449068e4654d3bca565ebe6d13aabebd7ed48d55e69ee15e271f0067dc3fbd39e5b1dd7692a4f70a8e9f8b763",
    "957fdef4a26efe70ebfb9f8d557e0fd5d20b02deb720fe7c6c32cc1823677d010990ff362af93d18bb0848bf393c36b9522d",
    "67b30a06c42a32bf1b9d24e079b2495be228310159df8726c9d9b5e4d0243507f4ff6d6892d465d1bdff83a149946a2f51b4",
    "1deb97252f3abf0f4f12ec7e1ce5c36f6393376f7715a0122067fffc61749229904c716f98e7d987ddd0c83c20941f28fb74",
    "725e43dac165b70bb69e787a77416dbd492c3ffb8b999bbc9475d17984c9a28706c2ab13456b739846099647e6b6a619abee",
    "24e8c2349f137d45fe2f7f68981f3546cbe9f67e4c32add17edf1ea9ffe9c38ac12e479662d603074ea944a118dcf821bdfa",
    "c6f4f6850a49170db23f7c53867755e4946918657b3f3b6df5334d7bafb2f8f3d6a2ba021beb40eb091e4a2b9fa8eedc521d",
    "fe6cbdeff396f9ede18bca166ad447862a84fe845f55deeede24f3d8f4c5abb2afaf6b2afbb8d77a4355eaa776bcca1ebc24",
    "ebddff7047d7e29712bdba69b4b69fb69f7079dedeb300c5e0f9c7c81d1d7f657452520198ff72b2b578338d4705521bca22",
    "27401ba23f4c223f7aab38e9a504707210939c084d2ad6bfcd85b6e04016a5c4016a7a31c0bce52b009d900aa859c6161d80",
    "46cdb695ce95438fc0236d99d102093a434db95d62ecf5b0fa07d6900634342796b6778427c61048837ac59b2d1a206ba853",
    "3e3c23b347d344b8a6ff66839674ba7c3ff9ec99d893df400dcd88f41a5904d4a020488ba02770adcdf03fcedbcb515d0ead",
    "ce48bcd5e0d0dcbd1478d8add98c4576eaf2cf20e16a40cfa6bf6a161c7936c1de3e1a2295d9af9d3fc0c387679879f5042d",
    "f6d73078c4cb5301720fa6cbd282cc09ba92ca9d02e336b58a030ac35afcef9e745d4d95039a408b1a1a1da3a59eb3bf11e9",
    "f2b5695e3fa173569309563e3475429abc852ba9c6af456abc378345bea5fc5ebdd890a3a2560252934f21e87d4cdda79aba",
    "7143aabc7e8a6a74a7dc192149f0f93628aaeee4950894b8aaecab4863bfe234acdc3ffa83723f029d32e85cc570d67bcaa0",
    "f1d10fe3f8c3867f23806fb41e90077f060e1c6e81232c9860e6b820e17be40e203763d84d3628d9646df7b0ee3b3f305782",
    "31bf99fbc02d2478010ccac40a2196892540388896e8ff0a5ff015efcfd32932678265d641c24fc81d406ee9b0db396edd68",
    "9ef5396fda109bf0bf8246740080fdef0a2813c5bf12c5b7d72648686c339406d27d9c7e7a818c63fe3100f6c102884325a8",
    "8871d1968d5679b816622ab0a605c0738d02f806ed5b445017a6e3d82721d4817704f0c61ad8860e97b508e0a34640bf3403",
    "9ed787c630954ebf1e64fe7a0aec37821593a131d6bd189e378685ced094712b045660388f7965153cdb04940763e0da7300",
    "2b9367e70560dd32f0b1005e84b03a1e9e64c3db10be86f001846576b057089b8360b2073ce606cb50a43ce18d2600fb42e0",
    "535778ce15663402b9c75d02d8d902880fad02b8b505e08333e1551a1c0d4da035ec85f829063cae2edbe4087d78ed2cdab1",
    "3702d992d70016dd01703e2c1a0b4772ef950a211fad5b2f0ad056ef77ab3e759c4ff9a400da0faf132937b49a91276ca12d",
    "2bfd07a8aa9064a0dab206d6d160b248364f0d7e86f0181d1e980e1f0af143f6f08c297cb21c7e82705317c093360961873e",
    "3d5b46f14b10cc6f1160f93c6c8f03ec990a2f69b2373bc2c145a3730106dbfe45e1ff17242ec3eadfcb1bea92df9eb8e137",
    "03455c7b2630b40b12864d4472879cb1dd90ebf76f78c85e183b16c08ce286da86b2fe4669579fdf932bc7d2b84ce04f867d",
    "61b52d593b4b80c16ece44a988e733664f0743dbdf90f543aefb0b9a82194f4ed565ff47b8219fa594e695008aee3f6ca5c3",
    "29ee2ef682695c610f96f4d3d395a30406fd71efe114c5f22d8795706ebc64afe12ddad46ed9afda2840bd758ff6f4e1125c",
    "dbdf629ca47d0003c90a6623f60f4242c8cce764780138b995e13c06088a7b68fc5c2010bd875b5d24660419cc6e23f60501",
    "c1c564c878ad7bb4d779224c85fb610e4c876d904b7e6206249043e1b15ad19a839c8610040227eaa3df100a8f3e04220fa0",
    "10e1dfc0594e245c85a2f584a1df4e6dd427fc5ca4f828219f0ada2a4b95255c014fa22a5021419b1265a00214796f180463",
    "374344221f70014ed02518c0cd1852069769c634a3b05a937014352e8aa4808a14090023e049e4cfa4c0640a2471d3093802",
    "128649fa9dc1348aede944d1379d3ca5ea9211120d8a4f49464815c800ae74245017684a3587a0597bb90c335cc02070f2b6",
    "9914e54b806bfc1eeba9380ba53e14a5904c3d79336f24c39387cd304451b83027d1301cf30f304c0a94a92d180266723b48",
    "f0e677fc26cea513f44c92d44227f0bf500cf2df8ac18b22a19344761269194ff91c7329ba0cc916fd8d58fa57b2d7fc5bf6",
    "5a5140b4efac3728f34f0853322b2719d28161df7086d1fd19992820110944a1a35ba0cc933244b809c96275451987a31a86",
    "8289525851321327c5d1c108c76f59c593aa4b499427091c33a320b124f7d49704ab92599080e2e4cda393b4571e9c060f6b",
    "7048e82b592eae68e50a417435909d8962163c4c488bfc5d864c561688442c2e85c81906ad2ec651863834b18478aa1770e5",
    "502899c390847d3e42fb1c2a07a490438a902b5a7fa3aa923921028e2467874453a849a94c962a5a89981212574297a01214",
    "e1fe7440e1ea440c29dec6c766f2a03f14a37ea13f05a85547028ca3da9647115d65a021f291785a022ba64fcf9c07949428",
    "e28eb284141b1eca134d89260966930e71f1b48032cafe9cb57424b0527409981f8ca42cbd9ec5ede826992728fbdee0d225",
    "21ccdfe48bc03a15e53343300aa01ab8643a58b6249411071cafa154a80a448b696a52f4584a3c425501c18963003d0ac243",
    "81e74801d0a308a839b01bca79a2e493c23024087e68ef463d5fdea450544fe2af5156351bafd8902e25712a0c24684826fc",
    "d12a6212b88e9eae4485e685d2af0e45a50466c770e61e3aa521a89dc352918c106808b4a43c9446eda07398a2131737d395",
    "518124a676f73cf63740ed1014d73f80c09ed195f31700e844d97208a0ec2ccc41db6010db7001a32a1d2fa1c84fee7f437c",
    "4fb6021a337d507c3372c184a913e630ed1c74d9bbd5cda6afd4cdd7fc5b1ba3412e60fde55628d39609ca50bb1343b63f93",
    "86da20d2e919be5e7ad7242c15d00663a10e65a6cf960c1b2dc6287811a0a9525659907a818360ba1fd266611c2a480e8450",
    "19354bdf2cd889218decd18ec7b6ca9867626150aceeff5fe9c75ee6fedf6d73a6011394eb06099b8820211bb903ba432ef1",
    "6c86c9ba2feea8fdbdb27a94ee97bbb6657e094b6addc22dfdb8345431b6a82c81c0ae9bf6b1f7b8d2d1ed939df056b620d3",
    "45e02f450b90a56b71e4677a86eed56108338334d0c67a84883bdd627a19692b6d67f6bb500a4eb877603b653cadcea1bcbc",
    "7cb73f0fe8d1245f26df4c16c107f7f53864068ea06549ced16838d9c7893643cb16ba08c890060249740eeae3e86e2f2f47",
    "1ac35825602888c6697c41796868b908002e57d764017369a33b2ed33237e9d65b2f216671bd3be6e46cae20fd689c56a54a",
    "037785cc8a2419710a3741b1046229711886e9b14a4ac42accb26fa01ea93f0328b1696f5d48bac554ff8121bad079877261",
    "7287f3ac7f68677fa2282d9cf2bd1c3a85253a10d52f9e8c2d5a345e1014c7e1d438a472496d5d86b471e12f89c364928954",
    "25fa34255f479a7b5a003d274813c89124daaf830ea7db54d4b5c6002058fc06700902e038447ac3348f44194024429a4c22",
    "04d866989514e449f2fe4630bb1350a036c9e01bc584a3fad3983599034557e5b1d5bcb153ef471613dabcfe0b8bde1bdf54",
    "4c3a33f815c37028e1028e199085e1a00c8e05e57b0b5e407520855cd00a3948bea5488eab4103b8898e36806a28838eb551",
    "3e1c74a163ade8ec4dea1b7b92838c830654bd72d0d10ed00cfba0fe6a545309d1ea4cf514ac91cf04f96c81396ac849fb32",
    "ce94f51113e048591d21ada338a1238e686b03c6a3b3e3295b27b6c8b7066c80a17c167f0cbfdce2a25eaf2e6b68e18f19da",
    "33087d620ab18828219a881e1d868ebe8e8b4e39bf47274dd745778ccec5b12e63432dc7b08c59ba24a6268d7f11fd6362b3",
    "b5d7cae88dd33fe8ed324be9eec274cecfd9bd76dbd42359ad4316e4a23dfff627f2338f94dfbf504b4d0081422ba424436c",
    "c8ca9cc546af2ca6fcb0c93910ed81c4444c8bc652203d7ae8ad24968a117a1a402e5a51ac22d61243b15a2a6aba006919c4",
    "0e6da24491cba39747c74704fb992f8ce310380f60a4fd8e1a115cc7be958e2dd0008c044b5f8c93ce4c18c98a8898be6cd1",
    "f2692151d1cb23e35350674403e03eb20c115c031eb85b82440d404b80be23d971cc749b8491a938c14147bcd8fed01345ef",
    "130a4d1de0a746b2e25058b828323826243896bc481d2cb1f41f1f2f278a83e1eb24e9e37db565d3e9619eb8e7caf0f080c8",
    "78b1572a9d9051076225b1ca560e578289e97736c9a4d3978ff4088e21945f30bcb93ecb768ab0cd403631020b9b74e93433",
    "8e9ee029d2c2870c9b440dfd7d81e2e5a9b56c5f7391889ed1ed23618c3d49131b7933bcb55976e8ba2991c1c19e110181c1",
    "223ac1e200df90da56f64e95b12761eaa58409b857707844584034a15001bd357c8bb78ab00c50e43f22a1ac2b2a3f3fcc86",
    "36641785e086d1bdd57d3b85fecbe7a207d3513c27bdac6b240ac0069b1c16101545a89c847c2dc5a999ee64c924c569859d",
    "ef1a99af1766c310ae8c9b161c10141c2961132c75c02fe4ddb9c4b693582c99dd7a60190a2ac186503b0ca902250b3b9cf6",
    "07392185e2db4f6c78ff2f5ab809f281d4bba5d5c397ee6285f3e4c960770f1f20504d825a2988ee808b215042b2461fb655",
    "4bbe9b21df3532c9f0a701ca848f04fd901533c91058194cb6984e0a2607fd1f5d2c276693402d2ac6bb69544c81009d561a",
    "0e8f8cb9fc3fc7fcef3461fb3f",
);

fn inflate(hex: &str) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(Cursor::new(unhex(hex)))
        .read_to_end(&mut out)
        .unwrap();
    out
}

/// abort-alloc-failed@empty (`memory allocation of 127543348822022 bytes failed`, the process dies; the fuzz mutant
/// of sample_base/empty.dwg above): the guard of an R2004+ section buffer compared the stated size with the declared
/// page count, and a negative count (an i32 cast to u64) made that bound 2^64 pages. The bound is the pages the map
/// lists, and no section buffer exceeds 0x7FFFFFC7 bytes.
#[test]
fn ac18_section_size_is_bounded_by_the_pages_the_map_lists() {
    let bytes = inflate(AC1032_NEGATIVE_PAGE_COUNT);
    finishes_within(60, move || {
        let _ = acadrust::DwgReader::from_stream_with_options(Cursor::new(bytes), acadrust::DwgReadOptions::failsafe()).read();
    });
}

// ---- R7 review: siblings of the fuzz findings (audit of the readers for counts, sizes and arithmetic taken from
// the file; synthetic inputs written here) ----

/// The objects of a DXF document of one OBJECTS-section record (`record` is its group pairs, ASCII).
fn read_objects(record: &str) -> CadDocument {
    let dxf = format!("  0\nSECTION\n  2\nOBJECTS\n{record}  0\nENDSEC\n  0\nEOF\n");
    failsafe_dxf(&dxf).expect("the failsafe read returns a document")
}

/// CONTEXTDATAMANAGER: a sub-manager entry count (91) of 100,000 with no entry after it pushed 100,000 default entries
/// per sub-manager (nested counts of a few bytes each made billions). The list ends where the file's values do.
#[test]
fn nested_class_object_counts_end_with_the_values() {
    use acadrust::objects::{ClassObjectData, ObjectType};
    let mut record = String::from("  0\nCONTEXTDATAMANAGER\n  5\nC1\n100\nAcDbContextDataManager\n340\n0\n 90\n3\n");
    for _ in 0..3 {
        record.push_str("340\n0\n 91\n100000\n");
    }
    let document = read_objects(&record);
    let manager = document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::ClassObject(class) => match &class.data {
                ClassObjectData::ContextDataManager(manager) => Some(manager.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("the manager is read");
    assert_eq!(manager.sub_managers.len(), 3);
    assert!(manager.sub_managers.iter().all(|sub| sub.entries.is_empty()));
}

/// ASSOC2DCONSTRAINTGROUP: the root node's connection count (90) of 100,000 at the end of the record pushed 100,000
/// default connections (and the node list of a damaged registry as many nodes). The record's end ends the lists.
#[test]
fn constraint_group_counts_end_with_the_record() {
    use acadrust::objects::{AssociativeData, ObjectType};
    let mut record =
        String::from("  0\nACDBASSOC2DCONSTRAINTGROUP\n  5\nC2\n100\nAcDbAssoc2dConstraintGroup\n 90\n2\n 70\n0\n");
    for _ in 0..3 {
        record.push_str(" 10\n0.0\n 20\n0.0\n 30\n0.0\n");
    }
    record.push_str("360\n0\n 90\n0\n 90\n100000\n 90\n1\n 90\n100000\n");
    let document = read_objects(&record);
    let group = document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Associative(assoc) => match &assoc.data {
                AssociativeData::ConstraintGroup(group) => Some(group.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("the constraint group is read");
    assert!(group.nodes.iter().map(|node| node.connections.len()).sum::<usize>() <= 1, "{:?}", group.nodes.len());
}

/// LAYER group 62 of -32768: `i16::abs` overflowed (ACadSharp's Math.Abs throws); the layer is read, off.
#[test]
fn layer_color_of_i16_min_is_read() {
    let dxf = "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n 70\n1\n  0\nLAYER\n  2\nL1\n 70\n0\n 62\n-32768\n  6\nCONTINUOUS\n\
  0\nENDTAB\n  0\nENDSEC\n  0\nEOF\n";
    let document = failsafe_dxf(dxf).expect("the failsafe read returns a document");
    let layer = document.layers.get("L1").expect("the layer is read");
    assert!(layer.flags.off);
}

/// XDATA 1004 (binary chunk) holding a multi-byte character: the hex digits were sliced out of the string by byte
/// offset, and a slice inside the character panicked.
#[test]
fn binary_xdata_with_a_multibyte_character_is_read() {
    let dxf = "  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  5\n2B\n  8\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n1001\nAPP\n1004\nAéFF\n\
  0\nENDSEC\n  0\nEOF\n";
    let document = failsafe_dxf(dxf).expect("the failsafe read returns a document");
    assert_eq!(document.entities().count(), 1);
}

/// A handle-map offset delta of -2^63 (nine 0x80 continuation bytes, then 0x41: value 2^63 with the sign bit): the
/// negation overflowed. C#'s unchecked negation wraps, as the reader does.
#[test]
fn signed_modular_char_of_2_pow_63_does_not_overflow() {
    use acadrust::io::dwg::dwg_stream_readers::handle_reader::read_handles;
    let mut chunk = vec![0x00, 13, 0x01];
    chunk.extend([0x80; 9]);
    chunk.extend([0x41, 0x00, 0x00]);
    let map = read_handles(&chunk).expect("a damaged chunk is read, not rejected");
    assert_eq!(map.get(&1), Some(&i64::MIN));
}

/// panic@src/compat/acadsharp/entities.rs:3276 (`fuzz-dxf.py --impl rust --seed 3`, a mutant of
/// sample_AC1027_ascii.dxf): a VIEWPORT size that is not finite (here 1e999, infinity) is written to the dump as the
/// string "Infinity", and the view read it back as a JSON number to derive ScaleFactor and ViewWidth (None.unwrap()).
#[cfg(feature = "acadsharp-compat")]
#[test]
fn viewport_of_infinite_size_is_dumped() {
    let dxf = "  0\nSECTION\n  2\nENTITIES\n  0\nVIEWPORT\n  5\n2C\n100\nAcDbEntity\n 67\n1\n  8\n0\n100\nAcDbViewport\n\
 10\n0.0\n 20\n0.0\n 30\n0.0\n 40\n1e999\n 41\n1e999\n 68\n1\n 69\n2\n 12\n0.0\n 22\n0.0\n 45\n1e999\n  0\nENDSEC\n  0\nEOF\n";
    let dump = dump_of("viewport-infinite.dxf", dxf.as_bytes());
    assert!(dump.is_object());
}
