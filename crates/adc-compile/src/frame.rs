//! 配置フレーム (05-schema.md §4.0)。導出規則は docs/placement-frames.md が正典。

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

pub(crate) fn normalize(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    scale(a, 1.0 / n)
}

/// 面ローカル座標系(右手系、z=外向き法線)
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

/// ワールドフレーム(ルートフィーチャー用)
pub(crate) fn world_frame() -> Frame {
    Frame {
        origin: [0.0, 0.0, 0.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        z: [0.0, 0.0, 1.0],
    }
}

/// Rodriguesの回転公式: ベクトル v を単位軸 k まわりに theta 回転
pub(crate) fn rotate_vec(v: [f64; 3], k: [f64; 3], theta: f64) -> [f64; 3] {
    let (s, c) = theta.sin_cos();
    let kxv = cross(k, v);
    let kdv = dot(k, v);
    add(
        add(scale(v, c), scale(kxv, s)),
        scale(k, kdv * (1.0 - c)),
    )
}

/// 点 p を軸(axis_origin, 単位方向 axis_dir)まわりに theta 回転
pub(crate) fn rotate_point_about_line(
    p: [f64; 3],
    axis_origin: [f64; 3],
    axis_dir: [f64; 3],
    theta: f64,
) -> [f64; 3] {
    add(axis_origin, rotate_vec(sub(p, axis_origin), axis_dir, theta))
}

/// フレーム全体を軸まわりに回転(Circularパターン用)
pub(crate) fn rotate_frame(f: &Frame, axis_origin: [f64; 3], axis_dir: [f64; 3], theta: f64) -> Frame {
    let k = normalize(axis_dir);
    Frame {
        origin: rotate_point_about_line(f.origin, axis_origin, k, theta),
        x: rotate_vec(f.x, k, theta),
        y: rotate_vec(f.y, k, theta),
        z: rotate_vec(f.z, k, theta),
    }
}

/// 面の生成時点の幾何(重心・外向き法線)からのフレーム導出
/// (docs/placement-frames.md):
/// origin=重心、z=外向き法線、x=基準軸(+X、ただしzと平行なら+Y)の面内射影、y=z×x
pub(crate) fn frame_from_origin_normal(origin: [f64; 3], normal: [f64; 3]) -> Frame {
    let z = normalize(normal);
    let x_ref = if dot(z, [1.0, 0.0, 0.0]).abs() < 1.0 - 1e-6 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let x = normalize(sub(x_ref, scale(z, dot(x_ref, z))));
    let y = cross(z, x);
    Frame { origin, x, y, z }
}
