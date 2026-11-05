macro_rules! impl_vector_ops {
    ($vector:ident, $($field:ident),+ $(,)?) => {
        impl std::ops::AddAssign for $vector {
            fn add_assign(&mut self, other: Self) {
                $(self.$field += other.$field;)+
            }
        }

        impl std::ops::SubAssign for $vector {
            fn sub_assign(&mut self, other: Self) {
                $(self.$field -= other.$field;)+
            }
        }

        impl std::ops::MulAssign<f64> for $vector {
            fn mul_assign(&mut self, scalar: f64) {
                $(self.$field *= scalar;)+
            }
        }

        impl std::ops::DivAssign<f64> for $vector {
            fn div_assign(&mut self, scalar: f64) {
                $(self.$field /= scalar;)+
            }
        }

        impl std::ops::Add for $vector {
            type Output = Self;

            fn add(mut self, other: Self) -> Self {
                self += other;
                self
            }
        }

        impl std::ops::Sub for $vector {
            type Output = Self;

            fn sub(mut self, other: Self) -> Self {
                self -= other;
                self
            }
        }

        impl std::ops::Mul<f64> for $vector {
            type Output = Self;

            fn mul(mut self, scalar: f64) -> Self {
                self *= scalar;
                self
            }
        }

        impl std::ops::Mul<$vector> for f64 {
            type Output = $vector;

            fn mul(self, mut vector: $vector) -> $vector {
                vector *= self;
                vector
            }
        }

        impl std::ops::Div<f64> for $vector {
            type Output = Self;

            fn div(mut self, scalar: f64) -> Self {
                self /= scalar;
                self
            }
        }

        impl std::ops::Neg for $vector {
            type Output = Self;

            fn neg(self) -> Self {
                Self { $($field: -self.$field),+ }
            }
        }

        impl std::fmt::Display for $vector {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let precision = formatter.precision().unwrap_or(6).max(1);
                formatter.write_str(concat!(stringify!($vector), "("))?;
                for (index, component) in [$(self.$field),+].into_iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    formatter.write_str(&crate::vector_ops::format_component(component, precision))?;
                }
                formatter.write_str(")")
            }
        }
    };
}

pub(crate) fn format_component(value: f64, precision: usize) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value.is_nan() {
        return format!("{sign}nan");
    }
    if value.is_infinite() {
        return format!("{sign}inf");
    }
    let fractional = precision - 1;
    let scientific = format!("{value:.fractional$e}");
    let (mantissa, exponent_text) = scientific.split_once('e').expect("scientific notation");
    let exponent: i32 = exponent_text.parse().expect("binary64 exponent");
    if exponent < -4 || (exponent >= 0 && exponent as usize >= precision) {
        let significant = if mantissa.contains('.') {
            mantissa.trim_end_matches('0').trim_end_matches('.')
        } else {
            mantissa
        };
        return format!("{significant}e{exponent:+03}");
    }
    let mut digits = mantissa.trim_start_matches('-').replace('.', "");
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let point = exponent + 1;
    if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!(
            "{sign}{digits}{}",
            "0".repeat(point as usize - digits.len())
        )
    } else {
        format!(
            "{sign}{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    }
}
