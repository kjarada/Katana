macro_rules! impl_matrix_access {
    ($matrix:ident, $width:literal) => {
        impl std::ops::Index<(usize, usize)> for $matrix {
            type Output = f64;

            fn index(&self, (row, column): (usize, usize)) -> &f64 {
                &self.data[row * $width + column]
            }
        }

        impl std::ops::IndexMut<(usize, usize)> for $matrix {
            fn index_mut(&mut self, (row, column): (usize, usize)) -> &mut f64 {
                &mut self.data[row * $width + column]
            }
        }

        impl std::fmt::Display for $matrix {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let precision = formatter.precision().unwrap_or(6).max(1);
                formatter.write_str(concat!(stringify!($matrix), "(["))?;
                for (row_index, row) in self.data.chunks_exact($width).enumerate() {
                    formatter.write_str(if row_index == 0 { "[" } else { ", [" })?;
                    for (column, value) in row.iter().enumerate() {
                        if column != 0 {
                            formatter.write_str(", ")?;
                        }
                        formatter
                            .write_str(&crate::vector_ops::format_component(*value, precision))?;
                    }
                    formatter.write_str("]")?;
                }
                formatter.write_str("])")
            }
        }
    };
}
