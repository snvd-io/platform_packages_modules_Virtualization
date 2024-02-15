// Copyright 2024, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Low-level libfdt_bindgen wrapper, easy to integrate safely in higher-level APIs.
//!
//! These traits decouple the safe libfdt C function calls from the representation of those
//! user-friendly higher-level types, allowing the trait to be shared between different ones,
//! adapted to their use-cases (e.g. alloc-based userspace or statically allocated no_std).

use core::ffi::{c_int, CStr};
use core::mem;
use core::ptr;

use crate::{fdt_err, fdt_err_expect_zero, fdt_err_or_option, FdtError, Phandle, Result};

// Function names are the C function names without the `fdt_` prefix.

/// Safe wrapper around `fdt_create_empty_tree()` (C function).
pub(crate) fn create_empty_tree(fdt: &mut [u8]) -> Result<()> {
    let len = fdt.len().try_into().unwrap();
    let fdt = fdt.as_mut_ptr().cast();
    // SAFETY: fdt_create_empty_tree() only write within the specified length,
    //          and returns error if buffer was insufficient.
    //          There will be no memory write outside of the given fdt.
    let ret = unsafe { libfdt_bindgen::fdt_create_empty_tree(fdt, len) };

    fdt_err_expect_zero(ret)
}

/// Safe wrapper around `fdt_check_full()` (C function).
pub(crate) fn check_full(fdt: &[u8]) -> Result<()> {
    let len = fdt.len();
    let fdt = fdt.as_ptr().cast();
    // SAFETY: Only performs read accesses within the limits of the slice. If successful, this
    // call guarantees to other unsafe calls that the header contains a valid totalsize (w.r.t.
    // 'len' i.e. the self.fdt slice) that those C functions can use to perform bounds
    // checking. The library doesn't maintain an internal state (such as pointers) between
    // calls as it expects the client code to keep track of the objects (DT, nodes, ...).
    let ret = unsafe { libfdt_bindgen::fdt_check_full(fdt, len) };

    fdt_err_expect_zero(ret)
}

/// Wrapper for the read-only libfdt.h functions.
///
/// # Safety
///
/// Implementors must ensure that at any point where a method of this trait is called, the
/// underlying type returns the bytes of a valid device tree (as validated by `check_full`)
/// through its `.as_fdt_slice` method.
pub(crate) unsafe trait Libfdt {
    /// Provides an immutable slice containing the device tree.
    ///
    /// The implementation must ensure that the size of the returned slice and
    /// `fdt_header::totalsize` match.
    fn as_fdt_slice(&self) -> &[u8];

    /// Safe wrapper around `fdt_path_offset_namelen()` (C function).
    fn path_offset_namelen(&self, path: &[u8]) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // *_namelen functions don't include the trailing nul terminator in 'len'.
        let len = path.len().try_into().map_err(|_| FdtError::BadPath)?;
        let path = path.as_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor) and the
        // function respects the passed number of characters.
        let ret = unsafe { libfdt_bindgen::fdt_path_offset_namelen(fdt, path, len) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_node_offset_by_phandle()` (C function).
    fn node_offset_by_phandle(&self, phandle: Phandle) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let phandle = phandle.into();
        // SAFETY: Accesses are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_node_offset_by_phandle(fdt, phandle) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_node_offset_by_compatible()` (C function).
    fn node_offset_by_compatible(&self, prev: c_int, compatible: &CStr) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let compatible = compatible.as_ptr();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_node_offset_by_compatible(fdt, prev, compatible) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_next_node()` (C function).
    fn next_node(&self, node: c_int, depth: usize) -> Result<Option<(c_int, usize)>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let mut depth = depth.try_into().unwrap();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_next_node(fdt, node, &mut depth) };

        match fdt_err_or_option(ret)? {
            Some(offset) if depth >= 0 => {
                let depth = depth.try_into().unwrap();
                Ok(Some((offset, depth)))
            }
            _ => Ok(None),
        }
    }

    /// Safe wrapper around `fdt_parent_offset()` (C function).
    ///
    /// Note that this function returns a `Err` when called on a root.
    fn parent_offset(&self, node: c_int) -> Result<c_int> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_parent_offset(fdt, node) };

        fdt_err(ret)
    }

    /// Safe wrapper around `fdt_supernode_atdepth_offset()` (C function).
    ///
    /// Note that this function returns a `Err` when called on a node at a depth shallower than
    /// the provided `depth`.
    fn supernode_atdepth_offset(&self, node: c_int, depth: usize) -> Result<c_int> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let depth = depth.try_into().unwrap();
        let nodedepth = ptr::null_mut();
        let ret =
            // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
            unsafe { libfdt_bindgen::fdt_supernode_atdepth_offset(fdt, node, depth, nodedepth) };

        fdt_err(ret)
    }

    /// Safe wrapper around `fdt_subnode_offset_namelen()` (C function).
    fn subnode_offset_namelen(&self, parent: c_int, name: &[u8]) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let namelen = name.len().try_into().unwrap();
        let name = name.as_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe { libfdt_bindgen::fdt_subnode_offset_namelen(fdt, parent, name, namelen) };

        fdt_err_or_option(ret)
    }
    /// Safe wrapper around `fdt_first_subnode()` (C function).
    fn first_subnode(&self, node: c_int) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_first_subnode(fdt, node) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_next_subnode()` (C function).
    fn next_subnode(&self, node: c_int) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_next_subnode(fdt, node) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_address_cells()` (C function).
    fn address_cells(&self, node: c_int) -> Result<usize> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe { libfdt_bindgen::fdt_address_cells(fdt, node) };

        Ok(fdt_err(ret)?.try_into().unwrap())
    }

    /// Safe wrapper around `fdt_size_cells()` (C function).
    fn size_cells(&self, node: c_int) -> Result<usize> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe { libfdt_bindgen::fdt_size_cells(fdt, node) };

        Ok(fdt_err(ret)?.try_into().unwrap())
    }

    /// Safe wrapper around `fdt_get_name()` (C function).
    fn get_name(&self, node: c_int) -> Result<&[u8]> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let mut len = 0;
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor). On success, the
        // function returns valid null terminating string and otherwise returned values are dropped.
        let name = unsafe { libfdt_bindgen::fdt_get_name(fdt, node, &mut len) };
        let len = usize::try_from(fdt_err(len)?).unwrap().checked_add(1).unwrap();

        get_slice_at_ptr(self.as_fdt_slice(), name.cast(), len).ok_or(FdtError::Internal)
    }

    /// Safe wrapper around `fdt_getprop_namelen()` (C function).
    fn getprop_namelen(&self, node: c_int, name: &[u8]) -> Result<Option<&[u8]>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let namelen = name.len().try_into().map_err(|_| FdtError::BadPath)?;
        let name = name.as_ptr().cast();
        let mut len = 0;
        let prop =
            // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor) and the
            // function respects the passed number of characters.
            unsafe { libfdt_bindgen::fdt_getprop_namelen(fdt, node, name, namelen, &mut len) };

        if let Some(len) = fdt_err_or_option(len)? {
            let len = usize::try_from(len).unwrap();
            let bytes = get_slice_at_ptr(self.as_fdt_slice(), prop.cast(), len);

            Ok(Some(bytes.ok_or(FdtError::Internal)?))
        } else {
            Ok(None)
        }
    }

    /// Safe wrapper around `fdt_get_property_by_offset()` (C function).
    fn get_property_by_offset(&self, offset: c_int) -> Result<&libfdt_bindgen::fdt_property> {
        let mut len = 0;
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let prop = unsafe { libfdt_bindgen::fdt_get_property_by_offset(fdt, offset, &mut len) };

        let data_len = fdt_err(len)?.try_into().unwrap();
        // TODO(stable_feature(offset_of)): mem::offset_of!(fdt_property, data).
        let data_offset = memoffset::offset_of!(libfdt_bindgen::fdt_property, data);
        let len = data_offset.checked_add(data_len).ok_or(FdtError::Internal)?;

        if !is_aligned(prop) || get_slice_at_ptr(self.as_fdt_slice(), prop.cast(), len).is_none() {
            return Err(FdtError::Internal);
        }

        // SAFETY: The pointer is properly aligned, struct is fully contained in the DT slice.
        let prop = unsafe { &*prop };

        if data_len != u32::from_be(prop.len).try_into().unwrap() {
            return Err(FdtError::BadLayout);
        }

        Ok(prop)
    }

    /// Safe wrapper around `fdt_first_property_offset()` (C function).
    fn first_property_offset(&self, node: c_int) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_first_property_offset(fdt, node) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_next_property_offset()` (C function).
    fn next_property_offset(&self, prev: c_int) -> Result<Option<c_int>> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_next_property_offset(fdt, prev) };

        fdt_err_or_option(ret)
    }

    /// Safe wrapper around `fdt_find_max_phandle()` (C function).
    fn find_max_phandle(&self) -> Result<Phandle> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        let mut phandle = 0;
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ret = unsafe { libfdt_bindgen::fdt_find_max_phandle(fdt, &mut phandle) };

        fdt_err_expect_zero(ret)?;

        phandle.try_into()
    }

    /// Safe wrapper around `fdt_string()` (C function).
    fn string(&self, offset: c_int) -> Result<&CStr> {
        let fdt = self.as_fdt_slice().as_ptr().cast();
        // SAFETY: Accesses (read-only) are constrained to the DT totalsize.
        let ptr = unsafe { libfdt_bindgen::fdt_string(fdt, offset) };
        let bytes =
            get_slice_from_ptr(self.as_fdt_slice(), ptr.cast()).ok_or(FdtError::Internal)?;

        CStr::from_bytes_until_nul(bytes).map_err(|_| FdtError::Internal)
    }
}

/// Wrapper for the read-write libfdt.h functions.
///
/// # Safety
///
/// Implementors must ensure that at any point where a method of this trait is called, the
/// underlying type returns the bytes of a valid device tree (as validated by `check_full`)
/// through its `.as_fdt_slice_mut` method.
///
/// Some methods may make previously returned values such as node or string offsets or phandles
/// invalid by modifying the device tree (e.g. by inserting or removing new nodes or properties).
/// As most methods take or return such values, instead of marking them all as unsafe, this trait
/// is marked as unsafe as implementors must ensure that methods that modify the validity of those
/// values are never called while the values are still in use.
pub(crate) unsafe trait LibfdtMut {
    /// Provides a mutable pointer to a buffer containing the device tree.
    ///
    /// The implementation must ensure that the size of the returned slice is at least
    /// `fdt_header::totalsize`, to allow for device tree growth.
    fn as_fdt_slice_mut(&mut self) -> &mut [u8];

    /// Safe wrapper around `fdt_nop_node()` (C function).
    fn nop_node(&mut self, node: c_int) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe { libfdt_bindgen::fdt_nop_node(fdt, node) };

        fdt_err_expect_zero(ret)
    }

    /// Safe wrapper around `fdt_add_subnode_namelen()` (C function).
    fn add_subnode_namelen(&mut self, node: c_int, name: &[u8]) -> Result<c_int> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let namelen = name.len().try_into().unwrap();
        let name = name.as_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe { libfdt_bindgen::fdt_add_subnode_namelen(fdt, node, name, namelen) };

        fdt_err(ret)
    }

    /// Safe wrapper around `fdt_setprop()` (C function).
    fn setprop(&mut self, node: c_int, name: &CStr, value: &[u8]) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        let len = value.len().try_into().map_err(|_| FdtError::BadValue)?;
        let value = value.as_ptr().cast();
        // SAFETY: New value size is constrained to the DT totalsize
        //          (validated by underlying libfdt).
        let ret = unsafe { libfdt_bindgen::fdt_setprop(fdt, node, name, value, len) };

        fdt_err_expect_zero(ret)
    }

    /// Safe wrapper around `fdt_setprop_placeholder()` (C function).
    fn setprop_placeholder(&mut self, node: c_int, name: &CStr, size: usize) -> Result<&mut [u8]> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        let len = size.try_into().unwrap();
        let mut data = ptr::null_mut();
        let ret =
            // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
            unsafe { libfdt_bindgen::fdt_setprop_placeholder(fdt, node, name, len, &mut data) };

        fdt_err_expect_zero(ret)?;

        get_mut_slice_at_ptr(self.as_fdt_slice_mut(), data.cast(), size).ok_or(FdtError::Internal)
    }

    /// Safe wrapper around `fdt_setprop_inplace()` (C function).
    fn setprop_inplace(&mut self, node: c_int, name: &CStr, value: &[u8]) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        let len = value.len().try_into().map_err(|_| FdtError::BadValue)?;
        let value = value.as_ptr().cast();
        // SAFETY: New value size is constrained to the DT totalsize
        //          (validated by underlying libfdt).
        let ret = unsafe { libfdt_bindgen::fdt_setprop_inplace(fdt, node, name, value, len) };

        fdt_err_expect_zero(ret)
    }

    /// Safe wrapper around `fdt_appendprop()` (C function).
    fn appendprop(&mut self, node: c_int, name: &CStr, value: &[u8]) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        let len = value.len().try_into().map_err(|_| FdtError::BadValue)?;
        let value = value.as_ptr().cast();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe { libfdt_bindgen::fdt_appendprop(fdt, node, name, value, len) };

        fdt_err_expect_zero(ret)
    }

    /// Safe wrapper around `fdt_appendprop_addrrange()` (C function).
    fn appendprop_addrrange(
        &mut self,
        parent: c_int,
        node: c_int,
        name: &CStr,
        addr: u64,
        size: u64,
    ) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor).
        let ret = unsafe {
            libfdt_bindgen::fdt_appendprop_addrrange(fdt, parent, node, name, addr, size)
        };

        fdt_err_expect_zero(ret)
    }

    /// Safe wrapper around `fdt_delprop()` (C function).
    fn delprop(&mut self, node: c_int, name: &CStr) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor) when the
        // library locates the node's property. Removing the property may shift the offsets of
        // other nodes and properties but the borrow checker should prevent this function from
        // being called when FdtNode instances are in use.
        let ret = unsafe { libfdt_bindgen::fdt_delprop(fdt, node, name) };

        fdt_err_expect_zero(ret)
    }

    /// Safe wrapper around `fdt_nop_property()` (C function).
    fn nop_property(&mut self, node: c_int, name: &CStr) -> Result<()> {
        let fdt = self.as_fdt_slice_mut().as_mut_ptr().cast();
        let name = name.as_ptr();
        // SAFETY: Accesses are constrained to the DT totalsize (validated by ctor) when the
        // library locates the node's property.
        let ret = unsafe { libfdt_bindgen::fdt_nop_property(fdt, node, name) };

        fdt_err_expect_zero(ret)
    }
}

pub(crate) fn get_slice_at_ptr(s: &[u8], p: *const u8, len: usize) -> Option<&[u8]> {
    let offset = get_slice_ptr_offset(s, p)?;

    s.get(offset..offset.checked_add(len)?)
}

fn get_mut_slice_at_ptr(s: &mut [u8], p: *mut u8, len: usize) -> Option<&mut [u8]> {
    let offset = get_slice_ptr_offset(s, p)?;

    s.get_mut(offset..offset.checked_add(len)?)
}

fn get_slice_from_ptr(s: &[u8], p: *const u8) -> Option<&[u8]> {
    s.get(get_slice_ptr_offset(s, p)?..)
}

fn get_slice_ptr_offset(s: &[u8], p: *const u8) -> Option<usize> {
    s.as_ptr_range().contains(&p).then(|| {
        // SAFETY: Both pointers are in bounds, derive from the same object, and size_of::<T>()=1.
        (unsafe { p.offset_from(s.as_ptr()) }) as usize
        // TODO(stable_feature(ptr_sub_ptr)): p.sub_ptr()
    })
}

// TODO(stable_feature(pointer_is_aligned)): p.is_aligned()
fn is_aligned<T>(p: *const T) -> bool {
    (p as usize) % mem::align_of::<T>() == 0
}