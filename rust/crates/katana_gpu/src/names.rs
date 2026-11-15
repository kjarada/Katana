//! The C++ `toString` overload set of this layer's enumerations (Program,
//! Expansion, RendererKind, GpuBackend, GpuDevice): one trait for all of
//! them, so that `to_string(device)` reads as the C++ does whichever
//! enumeration it is given - the convention katana_entity set.

/// The text a C++ `toString` overload gives an enumeration.
pub trait EnumName: Copy {
    fn name(self) -> &'static str;
}

/// The C++ `toString(...)` of any of this layer's enumerations.
pub fn to_string<T: EnumName>(value: T) -> &'static str {
    value.name()
}
