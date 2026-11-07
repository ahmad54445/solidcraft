//! Protocol v2 frames as csi-agent writes them (docs/09-agent-protocol.md).

use custom_features::transfer::{columns_f64, columns_u32, read_frame};

fn frame(header: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((header.len() as u32).to_le_bytes());
    out.extend(header.as_bytes());
    out.extend((payload.len() as u64).to_le_bytes());
    out.extend(payload);
    out
}

#[test]
fn reads_frames_one_after_another() {
    let joints: Vec<u8> = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut stream = frame(r#"{"v":2,"type":"progress","done":1,"total":3,"label":"Geometry"}"#, &[]);
    stream.extend(frame(r#"{"v":2,"type":"joints","count":2}"#, &joints));
    let mut r = stream.as_slice();
    let (h, p) = read_frame(&mut r).unwrap();
    assert_eq!(h["label"], "Geometry");
    assert!(p.is_empty());
    let (h, p) = read_frame(&mut r).unwrap();
    assert_eq!(h["type"], "joints");
    // columns x, y, z of 2 joints
    assert_eq!(columns_f64(&p, 2, 3).unwrap(), [vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
    assert!(read_frame(&mut r).is_err(), "end of stream without 'done' is an error");
}

#[test]
fn rejects_other_protocol_versions_and_bad_sizes() {
    let stream = frame(r#"{"v":3,"type":"done"}"#, &[]);
    assert!(read_frame(&mut stream.as_slice()).is_err());
    assert!(columns_u32(&[0u8; 7], 1, 2).is_err());
    assert_eq!(columns_u32(&[], 0, 3).unwrap(), [Vec::<u32>::new(), vec![], vec![]]);
    assert_eq!(columns_f64(&[], 0, 1).unwrap(), [Vec::<f64>::new()]);
}

#[test]
fn u32_columns() {
    let p: Vec<u8> = [0u32, 1, 7, u32::MAX].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(columns_u32(&p, 2, 2).unwrap(), [vec![0, 1], vec![7, u32::MAX]]);
}
