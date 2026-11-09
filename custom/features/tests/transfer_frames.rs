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

#[test]
fn local_axes_are_the_columns_of_the_csi_matrix() {
    use custom_features::transfer::local_axes;
    // CSiBridge 25.3.1, frame element 2-1: Global = M · Local (docs/08 §4.4)
    let m = [1.0, -0.0, 2.6e-16, 2.6e-16, 0.0, -1.0, 0.0, 1.0, 0.0];
    let [a1, a2, a3] = local_axes(&m).unwrap();
    assert_eq!(a1, [1.0, 2.6e-16, 0.0]); // (c0, c3, c6)
    assert_eq!(a2, [-0.0, 0.0, 1.0]); // (c1, c4, c7)
    assert_eq!(a3, [2.6e-16, -1.0, 0.0]); // (c2, c5, c8)
    assert!(local_axes(&[f64::NAN; 9]).is_none(), "a failed API call gives no axes");
}
