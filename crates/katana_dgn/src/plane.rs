//! How an arc, an ellipse or a text lies on the plan: the one reading of a 2D
//! element's rotation and of a 3D element's orientation quaternion, and the
//! quaternion the writer gives a turn, so that the reader and the writer
//! cannot disagree about which way an element faces.

use katana_math::numerics::DEG_TO_RAD;

use crate::codes::ANGLE_STEPS_PER_DEGREE;

/// A point of the design, in master units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Xyz {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

/// Which way an element's own x axis points on the plan, in radians
/// anticlockwise from east, and whether its plane is seen from below - its y
/// axis clockwise from its x axis, so that its angles run clockwise on the
/// plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InPlan {
    pub angle: f64,
    pub mirrored: bool,
}

/// A 2D element's rotation: an integer of 1/360000 degree (dgnread.cpp:697,
/// the arc's; :627 the ellipse's; :755 the text's), anticlockwise.
pub fn rotation_in_plan(steps: i32) -> InPlan {
    InPlan {
        angle: f64::from(steps) / ANGLE_STEPS_PER_DEGREE * DEG_TO_RAD,
        mirrored: false,
    }
}

/// A 3D element's orientation, four integers of 1/2^31 - w, x, y, z - or None
/// when its plane is tilted out of the plan, which a drawing on the plan
/// cannot hold.
///
/// dgnlib writes a turn of theta about Z as the quaternion (cos(-theta/2),
/// 0, 0, sin(-theta/2)) (dgnhelp.cpp:1084 DGNRotationToQuaternion): the
/// inverse turn. So the element's axes are the ROWS of the quaternion's
/// rotation matrix: its x axis (1 - 2(y^2 + z^2), 2(xy - wz), .) and its
/// normal's height 1 - 2(x^2 + y^2). That reading gives dgnlib's own
/// quaternion back as the turn it was made from, which is all that is
/// known of the format here; a quaternion that is all zeros, which no
/// rotation is, is read as no turn. A normal whose height is within 1e-12 of
/// 1 is in plan: a quaternion is held to 2^-31, whose rounding alone leaves
/// a plan element's normal within 1e-18 of upright, and the steepest tilt
/// passed as in plan (sqrt(2e-12), 1.4e-6 rad) moves a point of a 1 km
/// circle by 1.4 mm in height and by a nanometre on the plan.
pub fn quaternion_in_plan(quaternion: [i32; 4]) -> Option<InPlan> {
    let [w, x, y, z] = quaternion.map(f64::from);
    let length = (w * w + x * x + y * y + z * z).sqrt();
    if length == 0.0 {
        return Some(InPlan {
            angle: 0.0,
            mirrored: false,
        });
    }
    let (w, x, y, z) = (w / length, x / length, y / length, z / length);
    let normal_height = 1.0 - 2.0 * (x * x + y * y);
    if normal_height.abs() < 1.0 - 1e-12 {
        return None;
    }
    let along_x = 1.0 - 2.0 * (y * y + z * z);
    let along_y = 2.0 * (x * y - w * z);
    Some(InPlan {
        angle: along_y.atan2(along_x),
        mirrored: normal_height < 0.0,
    })
}

/// The quaternion of a turn of `radians` about Z, as dgnlib writes one
/// (dgnhelp.cpp:1084-1095), rounded where dgnlib truncates: its reading by
/// quaternion_in_plan is the turn again.
pub fn rotation_quaternion(radians: f64) -> [i32; 4] {
    const UNIT: f64 = 2_147_483_647.0; // 2^31 - 1, dgnhelp.cpp:1090
    let half = -radians / 2.0;
    [
        (half.cos() * UNIT).round() as i32,
        0,
        0,
        (half.sin() * UNIT).round() as i32,
    ]
}
