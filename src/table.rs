use core::{fmt, iter::FusedIterator, marker::PhantomData};

use crate::{
    raw::{
        Allocator, Bucket, Global, InsertSlot, RawDrain, RawExtractIf, RawIntoIter, RawIter,
        RawTable,
    },
    TryReserveError,
};

/// Low-level hash table with explicit hashing.
///
/// The primary use case for this type over [`HashMap`] or [`HashSet`] is to
/// support types that do not implement the [`Hash`] and [`Eq`] traits, but
/// instead require additional data not contained in the key itself to compute a
/// hash and compare two elements for equality.
///
/// Examples of when this can be useful include:
/// - An `IndexMap` implementation where indices into a `Vec` are stored as
///   elements in a `HashTable<usize>`. Hashing and comparing the elements
///   requires indexing the associated `Vec` to get the actual value referred to
///   by the index.
/// - Avoiding re-computing a hash when it is already known.
/// - Mutating the key of an element in a way that doesn't affect its hash.
///
/// To achieve this, `HashTable` methods that search for an element in the table
/// require a hash value and equality function to be explicitly passed in as
/// arguments. The method will then iterate over the elements with the given
/// hash and call the equality function on each of them, until a match is found.
///
/// In most cases, a `HashTable` will not be exposed directly in an API. It will
/// instead be wrapped in a helper type which handles the work of calculating
/// hash values and comparing elements.
///
/// Due to its low-level nature, this type provides fewer guarantees than
/// [`HashMap`] and [`HashSet`]. Specifically, the API allows you to shoot
/// yourself in the foot by having multiple elements with identical keys in the
/// table. The table itself will still function correctly and lookups will
/// arbitrarily return one of the matching elements. However you should avoid
/// doing this because it changes the runtime of hash table operations from
/// `O(1)` to `O(k)` where `k` is the number of duplicate entries.
///
/// [`HashMap`]: super::HashMap
/// [`HashSet`]: super::HashSet
pub struct HashTable<T, A = Global>
where
    A: Allocator,
{
    pub(crate) table: RawTable<T, A>,
}

impl<T> HashTable<T, Global> {
    /// Creates an empty `HashTable`.
    ///
    /// The hash table is initially created with a capacity of 0, so it will not allocate until it
    /// is first inserted into.
    ///
    /// # Examples
    ///
    /// ```
    /// use hashbrown::HashTable;
    /// let mut table: HashTable<&str> = HashTable::new();
    /// assert_eq!(table.len(), 0);
    /// assert_eq!(table.capacity(), 0);
    /// ```
    pub const fn new() -> Self {
        Self {
            table: RawTable::new(),
        }
    }

    /// Creates an empty `HashTable` with the specified capacity.
    ///
    /// The hash table will be able to hold at least `capacity` elements without
    /// reallocating. If `capacity` is 0, the hash table will not allocate.
    ///
    /// # Examples
    ///
    /// ```
    /// use hashbrown::HashTable;
    /// let mut table: HashTable<&str> = HashTable::with_capacity(10);
    /// assert_eq!(table.len(), 0);
    /// assert!(table.capacity() >= 10);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            table: RawTable::with_capacity(capacity),
        }
    }
}

impl<T, A> HashTable<T, A>
where
    A: Allocator,
{
    /// Creates an empty `HashTable` using the given allocator.
    ///
    /// The hash table is initially created with a capacity of 0, so it will not allocate until it
    /// is first inserted into.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use bumpalo::Bump;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let bump = Bump::new();
    /// let mut table = HashTable::new_in(&bump);
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// // The created HashTable holds none elements
    /// assert_eq!(table.len(), 0);
    ///
    /// // The created HashTable also doesn't allocate memory
    /// assert_eq!(table.capacity(), 0);
    ///
    /// // Now we insert element inside created HashTable
    /// table.insert_unchecked(hasher(&"One"), "One", hasher);
    /// // We can see that the HashTable holds 1 element
    /// assert_eq!(table.len(), 1);
    /// // And it also allocates some capacity
    /// assert!(table.capacity() > 1);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub const fn new_in(alloc: A) -> Self {
        Self {
            table: RawTable::new_in(alloc),
        }
    }

    /// Creates an empty `HashTable` with the specified capacity using the given allocator.
    ///
    /// The hash table will be able to hold at least `capacity` elements without
    /// reallocating. If `capacity` is 0, the hash table will not allocate.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use bumpalo::Bump;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let bump = Bump::new();
    /// let mut table = HashTable::with_capacity_in(5, &bump);
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// // The created HashTable holds none elements
    /// assert_eq!(table.len(), 0);
    /// // But it can hold at least 5 elements without reallocating
    /// let empty_map_capacity = table.capacity();
    /// assert!(empty_map_capacity >= 5);
    ///
    /// // Now we insert some 5 elements inside created HashTable
    /// table.insert_unchecked(hasher(&"One"), "One", hasher);
    /// table.insert_unchecked(hasher(&"Two"), "Two", hasher);
    /// table.insert_unchecked(hasher(&"Three"), "Three", hasher);
    /// table.insert_unchecked(hasher(&"Four"), "Four", hasher);
    /// table.insert_unchecked(hasher(&"Five"), "Five", hasher);
    ///
    /// // We can see that the HashTable holds 5 elements
    /// assert_eq!(table.len(), 5);
    /// // But its capacity isn't changed
    /// assert_eq!(table.capacity(), empty_map_capacity)
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn with_capacity_in(capacity: usize, alloc: A) -> Self {
        Self {
            table: RawTable::with_capacity_in(capacity, alloc),
        }
    }

    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.table.allocator()
    }

    /// Returns a reference to an entry in the table with the given hash and
    /// which satisfies the equality function passed.
    ///
    /// This method will call `eq` for all entries with the given hash, but may
    /// also call it for entries with a different hash. `eq` should only return
    /// true for the desired entry, at which point the search is stopped.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), 1, hasher);
    /// table.insert_unchecked(hasher(&2), 2, hasher);
    /// table.insert_unchecked(hasher(&3), 3, hasher);
    /// assert_eq!(table.find(hasher(&2), |&val| val == 2), Some(&2));
    /// assert_eq!(table.find(hasher(&4), |&val| val == 4), None);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn find(&self, hash: u64, eq: impl FnMut(&T) -> bool) -> Option<&T> {
        self.table
            .find(hash, eq)
            .map(|bucket| unsafe { bucket.as_ref() })
    }

    /// Returns a mutable reference to an entry in the table with the given hash
    /// and which satisfies the equality function passed.
    ///
    /// This method will call `eq` for all entries with the given hash, but may
    /// also call it for entries with a different hash. `eq` should only return
    /// true for the desired entry, at which point the search is stopped.
    ///
    /// When mutating an entry, you should ensure that it still retains the same
    /// hash value as when it was inserted, otherwise lookups of that entry may
    /// fail to find it.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), (1, "a"), |val| hasher(&val.0));
    /// if let Some(val) = table.find_mut(hasher(&1), |val| val.0 == 1) {
    ///     val.1 = "b";
    /// }
    /// assert_eq!(table.find(hasher(&1), |val| val.0 == 1), Some(&(1, "b")));
    /// assert_eq!(table.find(hasher(&2), |val| val.0 == 2), None);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn find_mut(&mut self, hash: u64, eq: impl FnMut(&T) -> bool) -> Option<&mut T> {
        self.table
            .find(hash, eq)
            .map(|bucket| unsafe { bucket.as_mut() })
    }

    /// Returns an `OccupiedEntry` for an entry in the table with the given hash
    /// and which satisfies the equality function passed.
    ///
    /// This can be used to remove the entry from the table. Call
    /// [`HashTable::entry`] instead if you wish to insert an entry if the
    /// lookup fails.
    ///
    /// This method will call `eq` for all entries with the given hash, but may
    /// also call it for entries with a different hash. `eq` should only return
    /// true for the desired entry, at which point the search is stopped.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), (1, "a"), |val| hasher(&val.0));
    /// if let Some(entry) = table.find_entry(hasher(&1), |val| val.0 == 1) {
    ///     entry.remove();
    /// }
    /// assert_eq!(table.find(hasher(&1), |val| val.0 == 1), None);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn find_entry(
        &mut self,
        hash: u64,
        eq: impl FnMut(&T) -> bool,
    ) -> Option<OccupiedEntry<'_, T, A>> {
        self.table.find(hash, eq).map(|bucket| OccupiedEntry {
            hash,
            bucket,
            table: &mut self.table,
        })
    }

    /// Returns an `Entry` for an entry in the table with the given hash
    /// and which satisfies the equality function passed.
    ///
    /// This can be used to remove the entry from the table, or insert a new
    /// entry with the given hash if one doesn't already exist.
    ///
    /// This method will call `eq` for all entries with the given hash, but may
    /// also call it for entries with a different hash. `eq` should only return
    /// true for the desired entry, at which point the search is stopped.
    ///
    /// This method may grow the table in preparation for an insertion. Call
    /// [`HashTable::find_entry`] if this is undesirable.
    ///
    /// `hasher` is called if entries need to be moved or copied to a new table.
    /// This must return the same hash value that each entry was inserted with.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::Entry;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), (1, "a"), |val| hasher(&val.0));
    /// if let Entry::Occupied(entry) = table.entry(hasher(&1), |val| val.0 == 1, |val| hasher(&val.0))
    /// {
    ///     entry.remove();
    /// }
    /// if let Entry::Vacant(entry) = table.entry(hasher(&2), |val| val.0 == 2, |val| hasher(&val.0)) {
    ///     entry.insert((2, "b"));
    /// }
    /// assert_eq!(table.find(hasher(&1), |val| val.0 == 1), None);
    /// assert_eq!(table.find(hasher(&2), |val| val.0 == 2), Some(&(2, "b")));
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn entry(
        &mut self,
        hash: u64,
        eq: impl FnMut(&T) -> bool,
        hasher: impl Fn(&T) -> u64,
    ) -> Entry<'_, T, A> {
        match self.table.find_or_find_insert_slot(hash, eq, hasher) {
            Ok(bucket) => Entry::Occupied(OccupiedEntry {
                hash,
                bucket,
                table: &mut self.table,
            }),
            Err(insert_slot) => Entry::Vacant(VacantEntry {
                hash,
                insert_slot,
                table: &mut self.table,
            }),
        }
    }

    /// Inserts an element into the `HashTable` with the given hash value, but
    /// without checking whether an equivalent element already exists within the
    /// table.
    ///
    /// This is
    ///
    /// `hasher` is called if entries need to be moved or copied to a new table.
    /// This must return the same hash value that each entry was inserted with.
    pub fn insert_unchecked(
        &mut self,
        hash: u64,
        value: T,
        hasher: impl Fn(&T) -> u64,
    ) -> OccupiedEntry<'_, T, A> {
        let bucket = self.table.insert(hash, value, hasher);
        OccupiedEntry {
            hash,
            bucket,
            table: &mut self.table,
        }
    }

    /// Clears the table, removing all values.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut v = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// v.insert_unchecked(hasher(&1), 1, hasher);
    /// v.clear();
    /// assert!(v.is_empty());
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn clear(&mut self) {
        self.table.clear();
    }

    /// Shrinks the capacity of the table as much as possible. It will drop
    /// down as much as possible while maintaining the internal rules
    /// and possibly leaving some space in accordance with the resize policy.
    ///
    /// `hasher` is called if entries need to be moved or copied to a new table.
    /// This must return the same hash value that each entry was inserted with.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::with_capacity(100);
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), 1, hasher);
    /// table.insert_unchecked(hasher(&2), 2, hasher);
    /// assert!(table.capacity() >= 100);
    /// table.shrink_to_fit(hasher);
    /// assert!(table.capacity() >= 2);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn shrink_to_fit(&mut self, hasher: impl Fn(&T) -> u64) {
        self.table.shrink_to(self.len(), hasher)
    }

    /// Shrinks the capacity of the table with a lower limit. It will drop
    /// down no lower than the supplied limit while maintaining the internal rules
    /// and possibly leaving some space in accordance with the resize policy.
    ///
    /// `hasher` is called if entries need to be moved or copied to a new table.
    /// This must return the same hash value that each entry was inserted with.
    ///
    /// Panics if the current capacity is smaller than the supplied
    /// minimum capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::with_capacity(100);
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), 1, hasher);
    /// table.insert_unchecked(hasher(&2), 2, hasher);
    /// assert!(table.capacity() >= 100);
    /// table.shrink_to(10, hasher);
    /// assert!(table.capacity() >= 10);
    /// table.shrink_to(0, hasher);
    /// assert!(table.capacity() >= 2);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn shrink_to(&mut self, min_capacity: usize, hasher: impl Fn(&T) -> u64) {
        self.table.shrink_to(min_capacity, hasher);
    }

    /// Reserves capacity for at least `additional` more elements to be inserted
    /// in the `HashTable`. The collection may reserve more space to avoid
    /// frequent reallocations.
    ///
    /// `hasher` is called if entries need to be moved or copied to a new table.
    /// This must return the same hash value that each entry was inserted with.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds [`isize::MAX`] bytes and [`abort`] the program
    /// in case of allocation error. Use [`try_reserve`](HashTable::try_reserve) instead
    /// if you want to handle memory allocation failure.
    ///
    /// [`isize::MAX`]: https://doc.rust-lang.org/std/primitive.isize.html
    /// [`abort`]: https://doc.rust-lang.org/alloc/alloc/fn.handle_alloc_error.html
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<i32> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.reserve(10, hasher);
    /// assert!(table.capacity() >= 10);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn reserve(&mut self, additional: usize, hasher: impl Fn(&T) -> u64) {
        self.table.reserve(additional, hasher)
    }

    /// Tries to reserve capacity for at least `additional` more elements to be inserted
    /// in the given `HashTable`. The collection may reserve more space to avoid
    /// frequent reallocations.
    ///
    /// `hasher` is called if entries need to be moved or copied to a new table.
    /// This must return the same hash value that each entry was inserted with.
    ///
    /// # Errors
    ///
    /// If the capacity overflows, or the allocator reports a failure, then an error
    /// is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<i32> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table
    ///     .try_reserve(10, hasher)
    ///     .expect("why is the test harness OOMing on 10 bytes?");
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn try_reserve(
        &mut self,
        additional: usize,
        hasher: impl Fn(&T) -> u64,
    ) -> Result<(), TryReserveError> {
        self.table.try_reserve(additional, hasher)
    }

    /// Returns the number of elements the table can hold without reallocating.
    ///
    /// # Examples
    ///
    /// ```
    /// use hashbrown::HashTable;
    /// let table: HashTable<i32> = HashTable::with_capacity(100);
    /// assert!(table.capacity() >= 100);
    /// ```
    pub fn capacity(&self) -> usize {
        self.table.capacity()
    }

    /// Returns the number of elements in the table.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// let mut v = HashTable::new();
    /// assert_eq!(v.len(), 0);
    /// v.insert_unchecked(hasher(&1), 1, hasher);
    /// assert_eq!(v.len(), 1);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// Returns `true` if the set contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// let mut v = HashTable::new();
    /// assert!(v.is_empty());
    /// v.insert_unchecked(hasher(&1), 1, hasher);
    /// assert!(!v.is_empty());
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// An iterator visiting all elements in arbitrary order.
    /// The iterator element type is `&'a T`.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&"a"), "b", hasher);
    /// table.insert_unchecked(hasher(&"b"), "b", hasher);
    ///
    /// // Will print in an arbitrary order.
    /// for x in table.iter() {
    ///     println!("{}", x);
    /// }
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            inner: unsafe { self.table.iter() },
            marker: PhantomData,
        }
    }

    /// An iterator visiting all elements in arbitrary order,
    /// with mutable references to the elements.
    /// The iterator element type is `&'a mut T`.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&1), 1, hasher);
    /// table.insert_unchecked(hasher(&2), 2, hasher);
    /// table.insert_unchecked(hasher(&3), 3, hasher);
    ///
    /// // Update all values
    /// for val in table.iter_mut() {
    ///     *val *= 2;
    /// }
    ///
    /// assert_eq!(table.len(), 3);
    /// let mut vec: Vec<i32> = Vec::new();
    ///
    /// for val in &table {
    ///     println!("val: {}", val);
    ///     vec.push(*val);
    /// }
    ///
    /// // The `Iter` iterator produces items in arbitrary order, so the
    /// // items must be sorted to test them against a sorted array.
    /// vec.sort_unstable();
    /// assert_eq!(vec, [2, 4, 6]);
    ///
    /// assert_eq!(table.len(), 3);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            inner: unsafe { self.table.iter() },
            marker: PhantomData,
        }
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// In other words, remove all elements `e` such that `f(&e)` returns `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// for x in 1..=6 {
    ///     table.insert_unchecked(hasher(&x), x, hasher);
    /// }
    /// table.retain(|&mut x| x % 2 == 0);
    /// assert_eq!(table.len(), 3);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn retain(&mut self, mut f: impl FnMut(&mut T) -> bool) {
        // Here we only use `iter` as a temporary, preventing use-after-free
        unsafe {
            for item in self.table.iter() {
                if !f(item.as_mut()) {
                    self.table.erase(item);
                }
            }
        }
    }

    /// Clears the set, returning all elements in an iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// for x in 1..=3 {
    ///     table.insert_unchecked(hasher(&x), x, hasher);
    /// }
    /// assert!(!table.is_empty());
    ///
    /// // print 1, 2, 3 in an arbitrary order
    /// for i in table.drain() {
    ///     println!("{}", i);
    /// }
    ///
    /// assert!(table.is_empty());
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn drain(&mut self) -> Drain<'_, T, A> {
        Drain {
            inner: self.table.drain(),
        }
    }

    /// Drains elements which are true under the given predicate,
    /// and returns an iterator over the removed items.
    ///
    /// In other words, move all elements `e` such that `f(&e)` returns `true` out
    /// into another iterator.
    ///
    /// If the returned `ExtractIf` is not exhausted, e.g. because it is dropped without iterating
    /// or the iteration short-circuits, then the remaining elements will be retained.
    /// Use [`retain()`] with a negated predicate if you do not need the returned iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// for x in 0..8 {
    ///     table.insert_unchecked(hasher(&x), x, hasher);
    /// }
    /// let drained: Vec<i32> = table.extract_if(|&mut v| v % 2 == 0).collect();
    ///
    /// let mut evens = drained.into_iter().collect::<Vec<_>>();
    /// let mut odds = table.into_iter().collect::<Vec<_>>();
    /// evens.sort();
    /// odds.sort();
    ///
    /// assert_eq!(evens, vec![0, 2, 4, 6]);
    /// assert_eq!(odds, vec![1, 3, 5, 7]);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn extract_if<F>(&mut self, f: F) -> ExtractIf<'_, T, F, A>
    where
        F: FnMut(&mut T) -> bool,
    {
        ExtractIf {
            f,
            inner: RawExtractIf {
                iter: unsafe { self.table.iter() },
                table: &mut self.table,
            },
        }
    }
}

impl<T, A> IntoIterator for HashTable<T, A>
where
    A: Allocator,
{
    type Item = T;
    type IntoIter = IntoIter<T, A>;

    fn into_iter(self) -> IntoIter<T, A> {
        IntoIter {
            inner: self.table.into_iter(),
        }
    }
}

impl<'a, T, A> IntoIterator for &'a HashTable<T, A>
where
    A: Allocator,
{
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<'a, T, A> IntoIterator for &'a mut HashTable<T, A>
where
    A: Allocator,
{
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;

    fn into_iter(self) -> IterMut<'a, T> {
        self.iter_mut()
    }
}

impl<T, A> Default for HashTable<T, A>
where
    A: Allocator + Default,
{
    fn default() -> Self {
        Self {
            table: Default::default(),
        }
    }
}

impl<T, A> Clone for HashTable<T, A>
where
    T: Clone,
    A: Allocator + Clone,
{
    fn clone(&self) -> Self {
        Self {
            table: self.table.clone(),
        }
    }
}

impl<T, A> fmt::Debug for HashTable<T, A>
where
    T: fmt::Debug,
    A: Allocator,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// A view into a single entry in a table, which may either be vacant or occupied.
///
/// This `enum` is constructed from the [`entry`] method on [`HashTable`].
///
/// [`HashTable`]: struct.HashTable.html
/// [`entry`]: struct.HashTable.html#method.entry
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "nightly")]
/// # fn test() {
/// use ahash::AHasher;
/// use hashbrown::hash_table::{Entry, HashTable, OccupiedEntry};
/// use std::hash::{BuildHasher, BuildHasherDefault};
///
/// let mut table = HashTable::new();
/// let hasher = BuildHasherDefault::<AHasher>::default();
/// let hasher = |val: &_| hasher.hash_one(val);
/// for x in ["a", "b", "c"] {
///     table.insert_unchecked(hasher(&x), x, hasher);
/// }
/// assert_eq!(table.len(), 3);
///
/// // Existing value (insert)
/// let entry: Entry<_> = table.entry(hasher(&"a"), |&x| x == "a", hasher);
/// let _raw_o: OccupiedEntry<_, _> = entry.insert("a");
/// assert_eq!(table.len(), 3);
/// // Nonexistent value (insert)
/// table.entry(hasher(&"d"), |&x| x == "d", hasher).insert("d");
///
/// // Existing value (or_insert)
/// table
///     .entry(hasher(&"b"), |&x| x == "b", hasher)
///     .or_insert("b");
/// // Nonexistent value (or_insert)
/// table
///     .entry(hasher(&"e"), |&x| x == "e", hasher)
///     .or_insert("e");
///
/// println!("Our HashTable: {:?}", table);
///
/// let mut vec: Vec<_> = table.iter().copied().collect();
/// // The `Iter` iterator produces items in arbitrary order, so the
/// // items must be sorted to test them against a sorted array.
/// vec.sort_unstable();
/// assert_eq!(vec, ["a", "b", "c", "d", "e"]);
/// # }
/// # fn main() {
/// #     #[cfg(feature = "nightly")]
/// #     test()
/// # }
/// ```
pub enum Entry<'a, T, A = Global>
where
    A: Allocator,
{
    /// An occupied entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::{Entry, HashTable, OccupiedEntry};
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// for x in ["a", "b"] {
    ///     table.insert_unchecked(hasher(&x), x, hasher);
    /// }
    ///
    /// match table.entry(hasher(&"a"), |&x| x == "a", hasher) {
    ///     Entry::Vacant(_) => unreachable!(),
    ///     Entry::Occupied(_) => {}
    /// }
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    Occupied(OccupiedEntry<'a, T, A>),

    /// A vacant entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::{Entry, HashTable, OccupiedEntry};
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table = HashTable::<&str>::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// match table.entry(hasher(&"a"), |&x| x == "a", hasher) {
    ///     Entry::Vacant(_) => {}
    ///     Entry::Occupied(_) => unreachable!(),
    /// }
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    Vacant(VacantEntry<'a, T, A>),
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for Entry<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Entry::Vacant(ref v) => f.debug_tuple("Entry").field(v).finish(),
            Entry::Occupied(ref o) => f.debug_tuple("Entry").field(o).finish(),
        }
    }
}

impl<'a, T, A> Entry<'a, T, A>
where
    A: Allocator,
{
    /// Sets the value of the entry, replacing any existing value if there is
    /// one, and returns an [`OccupiedEntry`].
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<&str> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// let entry = table
    ///     .entry(hasher(&"horseyland"), |&x| x == "horseyland", hasher)
    ///     .insert("horseyland");
    ///
    /// assert_eq!(entry.get(), &"horseyland");
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn insert(self, value: T) -> OccupiedEntry<'a, T, A> {
        match self {
            Entry::Occupied(mut entry) => {
                *entry.get_mut() = value;
                entry
            }
            Entry::Vacant(entry) => entry.insert(value),
        }
    }

    /// Ensures a value is in the entry by inserting if it was vacant.
    ///
    /// Returns an [`OccupiedEntry`] pointing to the now-occupied entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<&str> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// // nonexistent key
    /// table
    ///     .entry(hasher(&"poneyland"), |&x| x == "poneyland", hasher)
    ///     .or_insert("poneyland");
    /// assert!(table
    ///     .find(hasher(&"poneyland"), |&x| x == "poneyland")
    ///     .is_some());
    ///
    /// // existing key
    /// table
    ///     .entry(hasher(&"poneyland"), |&x| x == "poneyland", hasher)
    ///     .or_insert("poneyland");
    /// assert!(table
    ///     .find(hasher(&"poneyland"), |&x| x == "poneyland")
    ///     .is_some());
    /// assert_eq!(table.len(), 1);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn or_insert(self, default: T) -> OccupiedEntry<'a, T, A> {
        match self {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Ensures a value is in the entry by inserting the result of the default function if empty..
    ///
    /// Returns an [`OccupiedEntry`] pointing to the now-occupied entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<String> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// table
    ///     .entry(hasher("poneyland"), |x| x == "poneyland", |val| hasher(val))
    ///     .or_insert_with(|| "poneyland".to_string());
    ///
    /// assert!(table
    ///     .find(hasher(&"poneyland"), |x| x == "poneyland")
    ///     .is_some());
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn or_insert_with(self, default: impl FnOnce() -> T) -> OccupiedEntry<'a, T, A> {
        match self {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Provides in-place mutable access to an occupied entry before any
    /// potential inserts into the table.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<(&str, u32)> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// table
    ///     .entry(
    ///         hasher(&"poneyland"),
    ///         |&(x, _)| x == "poneyland",
    ///         |(k, _)| hasher(&k),
    ///     )
    ///     .and_modify(|(_, v)| *v += 1)
    ///     .or_insert(("poneyland", 42));
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&(k, _)| k == "poneyland"),
    ///     Some(&("poneyland", 42))
    /// );
    ///
    /// table
    ///     .entry(
    ///         hasher(&"poneyland"),
    ///         |&(x, _)| x == "poneyland",
    ///         |(k, _)| hasher(&k),
    ///     )
    ///     .and_modify(|(_, v)| *v += 1)
    ///     .or_insert(("poneyland", 42));
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&(k, _)| k == "poneyland"),
    ///     Some(&("poneyland", 43))
    /// );
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn and_modify(self, f: impl FnOnce(&mut T)) -> Self {
        match self {
            Entry::Occupied(mut entry) => {
                f(entry.get_mut());
                Entry::Occupied(entry)
            }
            Entry::Vacant(entry) => Entry::Vacant(entry),
        }
    }
}

/// A view into an occupied entry in a `HashTable`.
/// It is part of the [`Entry`] enum.
///
/// [`Entry`]: enum.Entry.html
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "nightly")]
/// # fn test() {
/// use ahash::AHasher;
/// use hashbrown::hash_table::{Entry, HashTable, OccupiedEntry};
/// use std::hash::{BuildHasher, BuildHasherDefault};
///
/// let mut table = HashTable::new();
/// let hasher = BuildHasherDefault::<AHasher>::default();
/// let hasher = |val: &_| hasher.hash_one(val);
/// for x in ["a", "b", "c"] {
///     table.insert_unchecked(hasher(&x), x, hasher);
/// }
/// assert_eq!(table.len(), 3);
///
/// let _entry_o: OccupiedEntry<_, _> = table.find_entry(hasher(&"a"), |&x| x == "a").unwrap();
/// assert_eq!(table.len(), 3);
///
/// // Existing key
/// match table.entry(hasher(&"a"), |&x| x == "a", hasher) {
///     Entry::Vacant(_) => unreachable!(),
///     Entry::Occupied(view) => {
///         assert_eq!(view.get(), &"a");
///     }
/// }
///
/// assert_eq!(table.len(), 3);
///
/// // Existing key (take)
/// match table.entry(hasher(&"c"), |&x| x == "c", hasher) {
///     Entry::Vacant(_) => unreachable!(),
///     Entry::Occupied(view) => {
///         assert_eq!(view.remove().0, "c");
///     }
/// }
/// assert_eq!(table.find(hasher(&"c"), |&x| x == "c"), None);
/// assert_eq!(table.len(), 2);
/// # }
/// # fn main() {
/// #     #[cfg(feature = "nightly")]
/// #     test()
/// # }
/// ```
pub struct OccupiedEntry<'a, T, A = Global>
where
    A: Allocator,
{
    hash: u64,
    bucket: Bucket<T>,
    table: &'a mut RawTable<T, A>,
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for OccupiedEntry<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OccupiedEntry")
            .field("value", self.get())
            .finish()
    }
}

impl<'a, T, A> OccupiedEntry<'a, T, A>
where
    A: Allocator,
{
    /// Takes the value out of the entry, and returns it along with a
    /// `VacantEntry` that can be used to insert another value with the same
    /// hash as the one that was just removed.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::Entry;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<&str> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// // The table is empty
    /// assert!(table.is_empty() && table.capacity() == 0);
    ///
    /// table.insert_unchecked(hasher(&"poneyland"), "poneyland", hasher);
    /// let capacity_before_remove = table.capacity();
    ///
    /// if let Entry::Occupied(o) = table.entry(hasher(&"poneyland"), |&x| x == "poneyland", hasher) {
    ///     assert_eq!(o.remove().0, "poneyland");
    /// }
    ///
    /// assert!(table
    ///     .find(hasher(&"poneyland"), |&x| x == "poneyland")
    ///     .is_none());
    /// // Now table hold none elements but capacity is equal to the old one
    /// assert!(table.len() == 0 && table.capacity() == capacity_before_remove);
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn remove(self) -> (T, VacantEntry<'a, T, A>) {
        let (val, slot) = unsafe { self.table.remove(self.bucket) };
        (
            val,
            VacantEntry {
                hash: self.hash,
                insert_slot: slot,
                table: self.table,
            },
        )
    }

    /// Gets a reference to the value in the entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::Entry;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<&str> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&"poneyland"), "poneyland", hasher);
    ///
    /// match table.entry(hasher(&"poneyland"), |&x| x == "poneyland", hasher) {
    ///     Entry::Vacant(_) => panic!(),
    ///     Entry::Occupied(entry) => assert_eq!(entry.get(), &"poneyland"),
    /// }
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn get(&self) -> &T {
        unsafe { self.bucket.as_ref() }
    }

    /// Gets a mutable reference to the value in the entry.
    ///
    /// If you need a reference to the `OccupiedEntry` which may outlive the
    /// destruction of the `Entry` value, see [`into_mut`].
    ///
    /// [`into_mut`]: #method.into_mut
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::Entry;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<(&str, u32)> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&"poneyland"), ("poneyland", 12), |(k, _)| hasher(&k));
    ///
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&(x, _)| x == "poneyland",),
    ///     Some(&("poneyland", 12))
    /// );
    ///
    /// if let Entry::Occupied(mut o) = table.entry(
    ///     hasher(&"poneyland"),
    ///     |&(x, _)| x == "poneyland",
    ///     |(k, _)| hasher(&k),
    /// ) {
    ///     o.get_mut().1 += 10;
    ///     assert_eq!(o.get().1, 22);
    ///
    ///     // We can use the same Entry multiple times.
    ///     o.get_mut().1 += 2;
    /// }
    ///
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&(x, _)| x == "poneyland",),
    ///     Some(&("poneyland", 24))
    /// );
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { self.bucket.as_mut() }
    }

    /// Converts the OccupiedEntry into a mutable reference to the value in the entry
    /// with a lifetime bound to the table itself.
    ///
    /// If you need multiple references to the `OccupiedEntry`, see [`get_mut`].
    ///
    /// [`get_mut`]: #method.get_mut
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::Entry;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<(&str, u32)> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    /// table.insert_unchecked(hasher(&"poneyland"), ("poneyland", 12), |(k, _)| hasher(&k));
    ///
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&(x, _)| x == "poneyland",),
    ///     Some(&("poneyland", 12))
    /// );
    ///
    /// let value: &mut (&str, u32);
    /// match table.entry(
    ///     hasher(&"poneyland"),
    ///     |&(x, _)| x == "poneyland",
    ///     |(k, _)| hasher(&k),
    /// ) {
    ///     Entry::Occupied(entry) => value = entry.into_mut(),
    ///     Entry::Vacant(_) => panic!(),
    /// }
    /// value.1 += 10;
    ///
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&(x, _)| x == "poneyland",),
    ///     Some(&("poneyland", 22))
    /// );
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn into_mut(self) -> &'a mut T {
        unsafe { self.bucket.as_mut() }
    }
}

/// A view into a vacant entry in a `HashTable`.
/// It is part of the [`Entry`] enum.
///
/// [`Entry`]: enum.Entry.html
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "nightly")]
/// # fn test() {
/// use ahash::AHasher;
/// use hashbrown::hash_table::{Entry, HashTable, VacantEntry};
/// use std::hash::{BuildHasher, BuildHasherDefault};
///
/// let mut table: HashTable<&str> = HashTable::new();
/// let hasher = BuildHasherDefault::<AHasher>::default();
/// let hasher = |val: &_| hasher.hash_one(val);
///
/// let entry_v: VacantEntry<_, _> = match table.entry(hasher(&"a"), |&x| x == "a", hasher) {
///     Entry::Vacant(view) => view,
///     Entry::Occupied(_) => unreachable!(),
/// };
/// entry_v.insert("a");
/// assert!(table.find(hasher(&"a"), |&x| x == "a").is_some() && table.len() == 1);
///
/// // Nonexistent key (insert)
/// match table.entry(hasher(&"b"), |&x| x == "b", hasher) {
///     Entry::Vacant(view) => {
///         view.insert("b");
///     }
///     Entry::Occupied(_) => unreachable!(),
/// }
/// assert!(table.find(hasher(&"b"), |&x| x == "b").is_some() && table.len() == 2);
/// # }
/// # fn main() {
/// #     #[cfg(feature = "nightly")]
/// #     test()
/// # }
/// ```
pub struct VacantEntry<'a, T, A = Global>
where
    A: Allocator,
{
    hash: u64,
    insert_slot: InsertSlot,
    table: &'a mut RawTable<T, A>,
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for VacantEntry<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VacantEntry")
    }
}

impl<'a, T, A> VacantEntry<'a, T, A>
where
    A: Allocator,
{
    /// Inserts a new element into the table with the hash that was used to
    /// obtain the `VacantEntry`.
    ///
    /// An `OccupiedEntry` is returned for the newly inserted element.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "nightly")]
    /// # fn test() {
    /// use ahash::AHasher;
    /// use hashbrown::hash_table::Entry;
    /// use hashbrown::HashTable;
    /// use std::hash::{BuildHasher, BuildHasherDefault};
    ///
    /// let mut table: HashTable<&str> = HashTable::new();
    /// let hasher = BuildHasherDefault::<AHasher>::default();
    /// let hasher = |val: &_| hasher.hash_one(val);
    ///
    /// if let Entry::Vacant(o) = table.entry(hasher(&"poneyland"), |&x| x == "poneyland", hasher) {
    ///     o.insert("poneyland");
    /// }
    /// assert_eq!(
    ///     table.find(hasher(&"poneyland"), |&x| x == "poneyland"),
    ///     Some(&"poneyland")
    /// );
    /// # }
    /// # fn main() {
    /// #     #[cfg(feature = "nightly")]
    /// #     test()
    /// # }
    /// ```
    pub fn insert(self, value: T) -> OccupiedEntry<'a, T, A> {
        let bucket = unsafe {
            self.table
                .insert_in_slot(self.hash, self.insert_slot, value)
        };
        OccupiedEntry {
            hash: self.hash,
            bucket,
            table: self.table,
        }
    }
}

/// An iterator over the entries of a `HashTable` in arbitrary order.
/// The iterator element type is `&'a T`.
///
/// This `struct` is created by the [`iter`] method on [`HashTable`]. See its
/// documentation for more.
///
/// [`iter`]: struct.HashTable.html#method.iter
/// [`HashTable`]: struct.HashTable.html
pub struct Iter<'a, T> {
    inner: RawIter<T>,
    marker: PhantomData<&'a T>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|bucket| unsafe { bucket.as_ref() })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<T> FusedIterator for Iter<'_, T> {}

/// A mutable iterator over the entries of a `HashTable` in arbitrary order.
/// The iterator element type is `&'a mut T`.
///
/// This `struct` is created by the [`iter_mut`] method on [`HashTable`]. See its
/// documentation for more.
///
/// [`iter_mut`]: struct.HashTable.html#method.iter_mut
/// [`HashTable`]: struct.HashTable.html
pub struct IterMut<'a, T> {
    inner: RawIter<T>,
    marker: PhantomData<&'a mut T>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|bucket| unsafe { bucket.as_mut() })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<T> FusedIterator for IterMut<'_, T> {}

/// An owning iterator over the entries of a `HashTable` in arbitrary order.
/// The iterator element type is `T`.
///
/// This `struct` is created by the [`into_iter`] method on [`HashTable`]
/// (provided by the [`IntoIterator`] trait). See its documentation for more.
/// The table cannot be used after calling that method.
///
/// [`into_iter`]: struct.HashTable.html#method.into_iter
/// [`HashTable`]: struct.HashTable.html
/// [`IntoIterator`]: https://doc.rust-lang.org/core/iter/trait.IntoIterator.html
pub struct IntoIter<T, A = Global>
where
    A: Allocator,
{
    inner: RawIntoIter<T, A>,
}

impl<T, A> Iterator for IntoIter<T, A>
where
    A: Allocator,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T, A> ExactSizeIterator for IntoIter<T, A>
where
    A: Allocator,
{
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<T, A> FusedIterator for IntoIter<T, A> where A: Allocator {}

/// A draining iterator over the items of a `HashTable`.
///
/// This `struct` is created by the [`drain`] method on [`HashTable`].
/// See its documentation for more.
///
/// [`HashTable`]: struct.HashTable.html
/// [`drain`]: struct.HashTable.html#method.drain
pub struct Drain<'a, T, A: Allocator = Global> {
    inner: RawDrain<'a, T, A>,
}

impl<T, A: Allocator> Drain<'_, T, A> {
    /// Returns a iterator of references over the remaining items.
    fn iter(&self) -> Iter<'_, T> {
        Iter {
            inner: self.inner.iter(),
            marker: PhantomData,
        }
    }
}

impl<T, A: Allocator> Iterator for Drain<'_, T, A> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<T, A: Allocator> ExactSizeIterator for Drain<'_, T, A> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}
impl<T, A: Allocator> FusedIterator for Drain<'_, T, A> {}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for Drain<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// A draining iterator over entries of a `HashTable` which don't satisfy the predicate `f`.
///
/// This `struct` is created by [`HashTable::extract_if`]. See its
/// documentation for more.
#[must_use = "Iterators are lazy unless consumed"]
pub struct ExtractIf<'a, T, F, A: Allocator = Global>
where
    F: FnMut(&mut T) -> bool,
{
    f: F,
    inner: RawExtractIf<'a, T, A>,
}

impl<T, F, A: Allocator> Iterator for ExtractIf<'_, T, F, A>
where
    F: FnMut(&mut T) -> bool,
{
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next(|val| (self.f)(val))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.inner.iter.size_hint().1)
    }
}

impl<T, F, A: Allocator> FusedIterator for ExtractIf<'_, T, F, A> where F: FnMut(&mut T) -> bool {}
