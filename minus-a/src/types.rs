//! -A Core type system.
//!
//! Types are inferred from `set` / `setc` initializers and from reuse
//! signatures, then stay fixed. `setc` makes the *value* mutable, never the
//! type.
//!
//! Core types: `int` (i64), `double` (f64), `string`, `bool` (`on` / `off`),
//! arrays, structs, enums, pointers, and `result<T>` (the value produced by a
//! fallible reuse). `mixed` is the element type of a heterogeneous array and
//! `unknown` marks a type the checker has not narrowed yet.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Double,
    String,
    Bool,
    Array(Box<Type>),
    /// Struct instance; `name` is canonical (`module.Name`), `args` are the
    /// type arguments of a generic struct.
    Struct {
        name: String,
        args: Vec<Type>,
    },
    /// Enum value.
    Enum(String),
    /// Raw pointer to a value of the inner type.
    Ptr(Box<Type>),
    /// Outcome of a fallible reuse: carries `ok` / `value` / `error`.
    Result(Box<Type>),
    /// Type parameter inside a generic declaration.
    Param(String),
    /// Element type of a heterogeneous array, or an unknown type.
    Mixed,
    Void,
    Unknown,
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Int => "int".to_string(),
            Type::Double => "double".to_string(),
            Type::String => "string".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Array(elem) => format!("[{}]", elem.name()),
            Type::Struct { name, args } => {
                if args.is_empty() {
                    name.clone()
                } else {
                    let inner: Vec<String> = args.iter().map(|a| a.name()).collect();
                    format!("{}<{}>", name, inner.join(", "))
                }
            }
            Type::Enum(name) => name.clone(),
            Type::Ptr(inner) => format!("ptr<{}>", inner.name()),
            Type::Result(inner) => format!("result<{}>", inner.name()),
            Type::Param(name) => name.clone(),
            Type::Mixed => "mixed".to_string(),
            Type::Void => "void".to_string(),
            Type::Unknown => "unknown".to_string(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Double)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }

    /// Can a value of type `other` be stored where `self` is expected?
    pub fn compatible_with(&self, other: &Type) -> bool {
        if matches!(self, Type::Unknown) || matches!(other, Type::Unknown) {
            return true;
        }
        if matches!(other, Type::Mixed) || matches!(self, Type::Mixed) {
            return true;
        }
        if self == other {
            return true;
        }
        match (self, other) {
            (Type::Double, Type::Int) => true,
            (Type::Array(target), Type::Array(source)) => {
                if matches!(**target, Type::Mixed) {
                    return true;
                }
                target.compatible_with(source)
            }
            (Type::Ptr(target), Type::Ptr(source)) => {
                if matches!(**target, Type::Mixed) || matches!(**source, Type::Mixed) {
                    return true;
                }
                target.compatible_with(source)
            }
            (Type::Struct { .. }, Type::Struct { .. }) => false,
            (Type::Result(a), Type::Result(b)) => a.compatible_with(b),
            _ => false,
        }
    }

    /// Numeric promotion: the result type of an arithmetic operation.
    pub fn promoted(a: &Type, b: &Type) -> Type {
        if matches!(a, Type::Double) || matches!(b, Type::Double) {
            Type::Double
        } else if matches!(a, Type::Unknown) || matches!(b, Type::Unknown) {
            Type::Unknown
        } else {
            Type::Int
        }
    }

    /// Values of these types live in a heap block or a static object and are
    /// therefore representable by an address.
    pub fn is_addressable_heap(&self) -> bool {
        matches!(
            self,
            Type::Array(_) | Type::Struct { .. } | Type::Result(_) | Type::Mixed
        )
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
