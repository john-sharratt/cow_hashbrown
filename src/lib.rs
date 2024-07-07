//! This crate is a Rust port of Google's high-performance [SwissTable] hash
//! map, adapted to make it a drop-in replacement for Rust's standard `HashMap`
//! and `HashSet` types.
//!
//! The original C++ version of [SwissTable] can be found [here], and this
//! [CppCon talk] gives an overview of how the algorithm works.
//!
//! [SwissTable]: https://abseil.io/blog/20180927-swisstables
//! [here]: https://github.com/abseil/abseil-cpp/blob/master/absl/container/internal/raw_hash_set.h
//! [CppCon talk]: https://www.youtube.com/watch?v=ncHmEUmJZf4

#![no_std]
#![cfg_attr(
    feature = "nightly",
    feature(
        test,
        core_intrinsics,
        dropck_eyepatch,
        min_specialization,
        extend_one,
        allocator_api,
        slice_ptr_get,
        maybe_uninit_array_assume_init,
        strict_provenance
    )
)]
#![allow(
    clippy::doc_markdown,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::option_if_let_else,
    clippy::redundant_else,
    clippy::manual_map,
    clippy::missing_safety_doc,
    clippy::missing_errors_doc
)]
#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![cfg_attr(feature = "nightly", warn(fuzzy_provenance_casts))]
#![cfg_attr(feature = "nightly", allow(internal_features))]

/// Default hasher for [`HashMap`], [`HashSet`] and [`HashTable`].
#[cfg(feature = "default-hasher")]
pub type DefaultHashBuilder = core::hash::BuildHasherDefault<ahash::AHasher>;

/// Dummy default hasher for [`HashMap`], [`HashSet`] and [`HashTable`].
#[cfg(not(feature = "default-hasher"))]
pub enum DefaultHashBuilder {}

#[cfg(test)]
#[macro_use]
extern crate std;

#[cfg_attr(test, macro_use)]
extern crate alloc;

#[cfg(feature = "nightly")]
#[cfg(doctest)]
doc_comment::doctest!("../README.md");

#[macro_use]
mod macros;

#[cfg(feature = "raw")]
/// Experimental and unsafe `RawTable` API. This module is only available if the
/// `raw` feature is enabled.
pub mod raw {
    // The RawTable API is still experimental and is not properly documented yet.
    #[allow(missing_docs)]
    #[path = "mod.rs"]
    mod inner;
    pub use inner::*;

    #[cfg(feature = "rayon")]
    /// [rayon]-based parallel iterator types for hash maps.
    /// You will rarely need to interact with it directly unless you have need
    /// to name one of the iterator types.
    ///
    /// [rayon]: https://docs.rs/rayon/1.0/rayon
    pub mod rayon {
        pub use crate::external_trait_impls::rayon::raw::*;
    }
}
#[cfg(not(feature = "raw"))]
mod raw;

mod external_trait_impls;
mod map;
#[cfg(feature = "rustc-internal-api")]
mod rustc_entry;
mod scopeguard;
//mod set;
//mod table;

pub mod hash_map {
    //! A hash map implemented with quadratic probing and SIMD lookup.
    pub use crate::map::*;

    #[cfg(feature = "rustc-internal-api")]
    pub use crate::rustc_entry::*;

    #[cfg(feature = "rayon")]
    /// [rayon]-based parallel iterator types for hash maps.
    /// You will rarely need to interact with it directly unless you have need
    /// to name one of the iterator types.
    ///
    /// [rayon]: https://docs.rs/rayon/1.0/rayon
    pub mod rayon {
        pub use crate::external_trait_impls::rayon::map::*;
    }
}
/*
pub mod hash_set {
    //! A hash set implemented as a `HashMap` where the value is `()`.
    pub use crate::set::*;

    #[cfg(feature = "rayon")]
    /// [rayon]-based parallel iterator types for hash sets.
    /// You will rarely need to interact with it directly unless you have need
    /// to name one of the iterator types.
    ///
    /// [rayon]: https://docs.rs/rayon/1.0/rayon
    pub mod rayon {
        pub use crate::external_trait_impls::rayon::set::*;
    }
}
pub mod hash_table {
    //! A hash table implemented with quadratic probing and SIMD lookup.
    pub use crate::table::*;

    #[cfg(feature = "rayon")]
    /// [rayon]-based parallel iterator types for hash tables.
    /// You will rarely need to interact with it directly unless you have need
    /// to name one of the iterator types.
    ///
    /// [rayon]: https://docs.rs/rayon/1.0/rayon
    pub mod rayon {
        pub use crate::external_trait_impls::rayon::table::*;
    }
}
*/

use core::ptr;

pub use crate::map::CowHashMap;
//pub use crate::set::CowHashSet;
//pub use crate::table::CowHashTable;

use arc_swap::AsRaw;
#[cfg(feature = "equivalent")]
pub use equivalent::Equivalent;

// This is only used as a fallback when building as part of `std`.
#[cfg(not(feature = "equivalent"))]
/// Key equivalence trait.
///
/// This trait defines the function used to compare the input value with the
/// map keys (or set values) during a lookup operation such as [`HashMap::get`]
/// or [`HashSet::contains`].
/// It is provided with a blanket implementation based on the
/// [`Borrow`](core::borrow::Borrow) trait.
///
/// # Correctness
///
/// Equivalent values must hash to the same value.
pub trait Equivalent<K: ?Sized> {
    /// Checks if this value is equivalent to the given key.
    ///
    /// Returns `true` if both values are equivalent, and `false` otherwise.
    ///
    /// # Correctness
    ///
    /// When this function returns `true`, both `self` and `key` must hash to
    /// the same value.
    fn equivalent(&self, key: &K) -> bool;
}

#[cfg(not(feature = "equivalent"))]
impl<Q: ?Sized, K: ?Sized> Equivalent<K> for Q
where
    Q: Eq,
    K: core::borrow::Borrow<Q>,
{
    fn equivalent(&self, key: &K) -> bool {
        self == key.borrow()
    }
}

/// The error type for `try_reserve` methods.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TryReserveError {
    /// Error due to the computed capacity exceeding the collection's maximum
    /// (usually `isize::MAX` bytes).
    CapacityOverflow,

    /// The memory allocator returned an error
    AllocError {
        /// The layout of the allocation request that failed.
        layout: alloc::alloc::Layout,
    },
}

/// Comparison of two pointer-like things.
// A and B are likely to *be* references, or thin wrappers around that. Calling that with extra
// reference is just annoying.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn ptr_eq<Base, A, B>(a: A, b: B) -> bool
where
    A: AsRaw<Base>,
    B: AsRaw<Base>,
{
    let a = a.as_raw();
    let b = b.as_raw();
    ptr::eq(a, b)
}
