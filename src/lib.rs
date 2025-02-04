//! A cross-platform Rust API for memory mapped buffers.

#![doc(html_root_url = "https://docs.rs/mapr/0.7.0")]

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::MmapInner;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::MmapInner;

use std::fmt;
use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::ops::{Deref, DerefMut};
use std::slice;
use std::usize;

/// A memory map builder, providing advanced options and flags for specifying memory map behavior.
///
/// `MmapOptions` can be used to create an anonymous memory map using [`map_anon()`], or a
/// file-backed memory map using one of [`map()`], [`map_mut()`], [`map_exec()`], or
/// [`map_copy()`].
///
/// ## Safety
///
/// All file-backed memory map constructors are marked `unsafe` because of the potential for
/// *Undefined Behavior* (UB) using the map if the underlying file is subsequently modified, in or
/// out of process. Applications must consider the risk and take appropriate precautions when
/// using file-backed maps. Solutions such as file permissions, locks or process-private (e.g.
/// unlinked) files exist but are platform specific and limited.
///
/// [`map_anon()`]: MmapOptions::map_anon()
/// [`map()`]: MmapOptions::map()
/// [`map_mut()`]: MmapOptions::map_mut()
/// [`map_exec()`]: MmapOptions::map_exec()
/// [`map_copy()`]: MmapOptions::map_copy()
#[derive(Clone, Debug, Default)]
pub struct MmapOptions {
    offset: u64,
    len: Option<usize>,
    stack: bool,
    locked: bool,
    private: bool,
    huge: u8,
    noreserve: bool,
}

impl MmapOptions {
    /// Creates a new set of options for configuring and creating a memory map.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::{MmapMut, MmapOptions};
    /// # use std::io::Result;
    ///
    /// # fn main() -> Result<()> {
    /// // Create a new memory map builder.
    /// let mut mmap_options = MmapOptions::new();
    ///
    /// // Configure the memory map builder using option setters, then create
    /// // a memory map using one of `mmap_options.map_anon`, `mmap_options.map`,
    /// // `mmap_options.map_mut`, `mmap_options.map_exec`, or `mmap_options.map_copy`:
    /// let mut mmap: MmapMut = mmap_options.len(36).map_anon()?;
    ///
    /// // Use the memory map:
    /// mmap.copy_from_slice(b"...data to copy to the memory map...");
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> MmapOptions {
        MmapOptions::default()
    }

    /// Configures the memory map to start at byte `offset` from the beginning of the file.
    ///
    /// This option has no effect on anonymous memory maps.
    ///
    /// By default, the offset is 0.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::MmapOptions;
    /// use std::fs::File;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mmap = unsafe {
    ///     MmapOptions::new()
    ///                 .offset(2)
    ///                 .map(&File::open("README.md")?)?
    /// };
    /// assert_eq!(&b"mapr"[..],
    ///            &mmap[..4]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn offset(&mut self, offset: u64) -> &mut Self {
        self.offset = offset;
        self
    }

    /// Configures the created memory mapped buffer to be `len` bytes long.
    ///
    /// This option is mandatory for anonymous memory maps.
    ///
    /// For file-backed memory maps, the length will default to the file length.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::MmapOptions;
    /// use std::fs::File;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mmap = unsafe {
    ///     MmapOptions::new()
    ///                 .len(6)
    ///                 .map(&File::open("README.md")?)?
    /// };
    /// assert_eq!(&b"# mapr"[..], &mmap[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn len(&mut self, len: usize) -> &mut Self {
        self.len = Some(len);
        self
    }

    /// Returns the configured length, or the length of the provided file.
    fn get_len(&self, file: &File) -> Result<usize> {
        self.len.map(Ok).unwrap_or_else(|| {
            let len = file.metadata()?.len() - self.offset;
            if len > (usize::MAX as u64) {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "memory map length overflows usize",
                ));
            }
            Ok(len as usize)
        })
    }

    /// Configures the anonymous memory map to be suitable for a process or thread stack.
    ///
    /// This option corresponds to the `MAP_STACK` flag on Linux.
    ///
    /// This option has no effect on file-backed memory maps.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::MmapOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let stack = MmapOptions::new().stack().len(4096).map_anon();
    /// # Ok(())
    /// # }
    /// ```
    pub fn stack(&mut self) -> &mut Self {
        self.stack = true;
        self
    }

    /// Configures the memory map to be locked using.
    ///
    /// This option corresponds to the `MAP_LOCKED` flag on Linux, and has no effect on Window and MacOS.
    ///
    /// Note this requires privileged access.
    pub fn lock(&mut self) -> &mut Self {
        self.locked = true;
        self
    }

    /// Configures the memory map to be private.
    ///
    /// This option corresponds to the `MAP_PRIVATE` flag on Linux.
    pub fn private(&mut self) -> &mut Self {
        self.private = true;
        self
    }

    pub fn huge(&mut self, huge: u8) -> &mut Self {
        self.huge = huge;
        self
    }

    pub fn noreserve(&mut self) -> &mut Self {
        self.noreserve = true;
        self
    }

    /// Creates a read-only memory map backed by a file.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with read permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::MmapOptions;
    /// use std::fs::File;
    /// use std::io::Read;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mut file = File::open("README.md")?;
    ///
    /// let mut contents = Vec::new();
    /// file.read_to_end(&mut contents)?;
    ///
    /// let mmap = unsafe {
    ///     MmapOptions::new().map(&file)?
    /// };
    ///
    /// assert_eq!(&contents[..], &mmap[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn map(&self, file: &File) -> Result<Mmap> {
        MmapInner::map(self.get_len(file)?, file, self.offset, self.locked, self.private, self.huge, self.noreserve).map(|inner| Mmap { inner: inner })
    }

    /// Creates a readable and executable memory map backed by a file.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with read permissions.
    pub unsafe fn map_exec(&self, file: &File) -> Result<Mmap> {
        MmapInner::map_exec(self.get_len(file)?, file, self.offset, self.locked, self.private, self.huge, self.noreserve)
            .map(|inner| Mmap { inner: inner })
    }

    /// Creates a writeable memory map backed by a file.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with read and write permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use std::fs::OpenOptions;
    /// use std::path::PathBuf;
    ///
    /// use mapr::MmapOptions;
    /// #
    /// # fn main() -> std::io::Result<()> {
    /// # let tempdir = tempdir::TempDir::new("mmap")?;
    /// let path: PathBuf = /* path to file */
    /// #   tempdir.path().join("map_mut");
    /// let file = OpenOptions::new().read(true).write(true).create(true).open(&path)?;
    /// file.set_len(13)?;
    ///
    /// let mut mmap = unsafe {
    ///     MmapOptions::new().map_mut(&file)?
    /// };
    ///
    /// mmap.copy_from_slice(b"Hello, world!");
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn map_mut(&self, file: &File) -> Result<MmapMut> {
        MmapInner::map_mut(self.get_len(file)?, file, self.offset, self.locked, self.private, self.huge, self.noreserve)
            .map(|inner| MmapMut { inner: inner })
    }

    /// Creates a copy-on-write memory map backed by a file.
    ///
    /// Data written to the memory map will not be visible by other processes,
    /// and will not be carried through to the underlying file.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with writable permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::MmapOptions;
    /// use std::fs::File;
    /// use std::io::Write;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let file = File::open("README.md")?;
    /// let mut mmap = unsafe { MmapOptions::new().map_copy(&file)? };
    /// (&mut mmap[..]).write_all(b"Hello, world!")?;
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn map_copy(&self, file: &File) -> Result<MmapMut> {
        MmapInner::map_copy(self.get_len(file)?, file, self.offset, self.locked, self.huge, self.noreserve)
            .map(|inner| MmapMut { inner: inner })
    }

    /// Creates an anonymous memory map.
    ///
    /// Note: the memory map length must be configured to be greater than 0 before creating an
    /// anonymous memory map using `MmapOptions::len()`.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails.
    pub fn map_anon(&self) -> Result<MmapMut> {
        MmapInner::map_anon(self.len.unwrap_or(0), self.stack, self.locked, self.private, self.huge, self.noreserve).map(|inner| MmapMut { inner: inner })
    }
}

/// A handle to an immutable memory mapped buffer.
///
/// A `Mmap` may be backed by a file, or it can be anonymous map, backed by volatile memory. Use
/// [`MmapOptions`] or [`map()`] to create a file-backed memory map. To create an immutable
/// anonymous memory map, first create a mutable anonymous memory map, and then make it immutable
/// with [`MmapMut::make_read_only()`].
///
/// A file backed `Mmap` is created by `&File` reference, and will remain valid even after the
/// `File` is dropped. In other words, the `Mmap` handle is completely independent of the `File`
/// used to create it. For consistency, on some platforms this is achieved by duplicating the
/// underlying file handle. The memory will be unmapped when the `Mmap` handle is dropped.
///
/// Dereferencing and accessing the bytes of the buffer may result in page faults (e.g. swapping
/// the mapped pages into physical memory) though the details of this are platform specific.
///
/// `Mmap` is [`Sync`](std::marker::Sync) and [`Send`](std::marker::Send).
///
/// ## Safety
///
/// All file-backed memory map constructors are marked `unsafe` because of the potential for
/// *Undefined Behavior* (UB) using the map if the underlying file is subsequently modified, in or
/// out of process. Applications must consider the risk and take appropriate precautions when using
/// file-backed maps. Solutions such as file permissions, locks or process-private (e.g. unlinked)
/// files exist but are platform specific and limited.
///
/// ## Example
///
/// ```
/// use mapr::MmapOptions;
/// use std::io::Write;
/// use std::fs::File;
///
/// # fn main() -> std::io::Result<()> {
/// let file = File::open("README.md")?;
/// let mmap = unsafe { MmapOptions::new().map(&file)? };
/// assert_eq!(b"# mapr", &mmap[..6]);
/// # Ok(())
/// # }
/// ```
///
/// See [`MmapMut`] for the mutable version.
///
/// [`map()`]: Mmap::map()
pub struct Mmap {
    inner: MmapInner,
}

impl Mmap {
    /// Creates a read-only memory map backed by a file.
    ///
    /// This is equivalent to calling `MmapOptions::new().map(file)`.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with read permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use std::fs::File;
    /// use std::io::Read;
    ///
    /// use mapr::Mmap;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mut file = File::open("README.md")?;
    ///
    /// let mut contents = Vec::new();
    /// file.read_to_end(&mut contents)?;
    ///
    /// let mmap = unsafe { Mmap::map(&file)?  };
    ///
    /// assert_eq!(&contents[..], &mmap[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn map(file: &File) -> Result<Mmap> {
        MmapOptions::new().map(file)
    }

    /// Transition the memory map to be writable.
    ///
    /// If the memory map is file-backed, the file must have been opened with write permissions.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with writable permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use mapr::Mmap;
    /// use std::ops::DerefMut;
    /// use std::io::Write;
    /// # use std::fs::OpenOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tempdir = tempdir::TempDir::new("mmap")?;
    /// let file = /* file opened with write permissions */
    /// #          OpenOptions::new()
    /// #                      .read(true)
    /// #                      .write(true)
    /// #                      .create(true)
    /// #                      .open(tempdir.path()
    /// #                      .join("make_mut"))?;
    /// # file.set_len(128)?;
    /// let mmap = unsafe { Mmap::map(&file)? };
    /// // ... use the read-only memory map ...
    /// let mut mut_mmap = mmap.make_mut()?;
    /// mut_mmap.deref_mut().write_all(b"hello, world!")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn make_mut(mut self) -> Result<MmapMut> {
        self.inner.make_mut()?;
        Ok(MmapMut { inner: self.inner })
    }

    /// Uses `mlock` to lock the whole memory map into RAM.
    ///
    /// Note this requires privileged access.
    #[cfg(unix)]
    pub fn mlock(&mut self) -> Result<()> {
        self.inner.mlock()?;
        
        Ok(())
    }

    /// Uses `munlock` to unlock the whole memory map.
    ///
    /// Note this requires privileged access.
    #[cfg(unix)]
    pub fn munlock(&mut self) -> Result<()> {
        self.inner.munlock()?;
        
        Ok(())
    }
}

impl Deref for Mmap {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.inner.ptr(), self.inner.len()) }
    }
}

impl AsRef<[u8]> for Mmap {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl fmt::Debug for Mmap {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Mmap")
            .field("ptr", &self.as_ptr())
            .field("len", &self.len())
            .finish()
    }
}

/// A handle to a mutable memory mapped buffer.
///
/// A file-backed `MmapMut` buffer may be used to read from or write to a file. An anonymous
/// `MmapMut` buffer may be used any place that an in-memory byte buffer is needed. Use
/// [`MmapMut::map_mut()`] and [`MmapMut::map_anon()`] to create a mutable memory map of the
/// respective types, or [`MmapOptions::map_mut()`] and [`MmapOptions::map_anon()`] if non-default
/// options are required.
///
/// A file backed `MmapMut` is created by `&File` reference, and will remain valid even after the
/// `File` is dropped. In other words, the `MmapMut` handle is completely independent of the `File`
/// used to create it. For consistency, on some platforms this is achieved by duplicating the
/// underlying file handle. The memory will be unmapped when the `MmapMut` handle is dropped.
///
/// Dereferencing and accessing the bytes of the buffer may result in page faults (e.g. swapping
/// the mapped pages into physical memory) though the details of this are platform specific.
///
/// `Mmap` is [`Sync`](std::marker::Sync) and [`Send`](std::marker::Send).
///
/// See [`Mmap`] for the immutable version.
///
/// ## Safety
///
/// All file-backed memory map constructors are marked `unsafe` because of the potential for
/// *Undefined Behavior* (UB) using the map if the underlying file is subsequently modified, in or
/// out of process. Applications must consider the risk and take appropriate precautions when using
/// file-backed maps. Solutions such as file permissions, locks or process-private (e.g. unlinked)
/// files exist but are platform specific and limited.
pub struct MmapMut {
    inner: MmapInner,
}

impl MmapMut {
    /// Creates a writeable memory map backed by a file.
    ///
    /// This is equivalent to calling `MmapOptions::new().map_mut(file)`.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file is not open with read and write permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use std::fs::OpenOptions;
    /// use std::path::PathBuf;
    ///
    /// use mapr::MmapMut;
    /// #
    /// # fn main() -> std::io::Result<()> {
    /// # let tempdir = tempdir::TempDir::new("mmap")?;
    /// let path: PathBuf = /* path to file */
    /// #   tempdir.path().join("map_mut");
    /// let file = OpenOptions::new()
    ///                        .read(true)
    ///                        .write(true)
    ///                        .create(true)
    ///                        .open(&path)?;
    /// file.set_len(13)?;
    ///
    /// let mut mmap = unsafe { MmapMut::map_mut(&file)? };
    ///
    /// mmap.copy_from_slice(b"Hello, world!");
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn map_mut(file: &File) -> Result<MmapMut> {
        MmapOptions::new().map_mut(file)
    }

    /// Creates an anonymous memory map.
    ///
    /// This is equivalent to calling `MmapOptions::new().len(length).map_anon()`.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails.
    pub fn map_anon(length: usize) -> Result<MmapMut> {
        MmapOptions::new().len(length).map_anon()
    }

    /// Flushes outstanding memory map modifications to disk.
    ///
    /// When this method returns with a non-error result, all outstanding changes to a file-backed
    /// memory map are guaranteed to be durably stored. The file's metadata (including last
    /// modification timestamp) may not be updated.
    ///
    /// # Example
    ///
    /// ```
    /// use std::fs::OpenOptions;
    /// use std::io::Write;
    /// use std::path::PathBuf;
    ///
    /// use mapr::MmapMut;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tempdir = tempdir::TempDir::new("mmap")?;
    /// let path: PathBuf = /* path to file */
    /// #   tempdir.path().join("flush");
    /// let file = OpenOptions::new().read(true).write(true).create(true).open(&path)?;
    /// file.set_len(128)?;
    ///
    /// let mut mmap = unsafe { MmapMut::map_mut(&file)? };
    ///
    /// (&mut mmap[..]).write_all(b"Hello, world!")?;
    /// mmap.flush()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn flush(&self) -> Result<()> {
        let len = self.len();
        self.inner.flush(0, len)
    }

    /// Asynchronously flushes outstanding memory map modifications to disk.
    ///
    /// This method initiates flushing modified pages to durable storage, but it will not wait for
    /// the operation to complete before returning. The file's metadata (including last
    /// modification timestamp) may not be updated.
    pub fn flush_async(&self) -> Result<()> {
        let len = self.len();
        self.inner.flush_async(0, len)
    }

    /// Flushes outstanding memory map modifications in the range to disk.
    ///
    /// The offset and length must be in the bounds of the memory map.
    ///
    /// When this method returns with a non-error result, all outstanding changes to a file-backed
    /// memory in the range are guaranteed to be durable stored. The file's metadata (including
    /// last modification timestamp) may not be updated. It is not guaranteed the only the changes
    /// in the specified range are flushed; other outstanding changes to the memory map may be
    /// flushed as well.
    pub fn flush_range(&self, offset: usize, len: usize) -> Result<()> {
        self.inner.flush(offset, len)
    }

    /// Asynchronously flushes outstanding memory map modifications in the range to disk.
    ///
    /// The offset and length must be in the bounds of the memory map.
    ///
    /// This method initiates flushing modified pages to durable storage, but it will not wait for
    /// the operation to complete before returning. The file's metadata (including last
    /// modification timestamp) may not be updated. It is not guaranteed that the only changes
    /// flushed are those in the specified range; other outstanding changes to the memory map may
    /// be flushed as well.
    pub fn flush_async_range(&self, offset: usize, len: usize) -> Result<()> {
        self.inner.flush_async(offset, len)
    }

    /// Returns an immutable version of this memory mapped buffer.
    ///
    /// If the memory map is file-backed, the file must have been opened with read permissions.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file has not been opened with read permissions.
    ///
    /// # Example
    ///
    /// ```
    /// use std::io::Write;
    /// use std::path::PathBuf;
    ///
    /// use mapr::{Mmap, MmapMut};
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mut mmap = MmapMut::map_anon(128)?;
    ///
    /// (&mut mmap[..]).write(b"Hello, world!")?;
    ///
    /// let mmap: Mmap = mmap.make_read_only()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn make_read_only(mut self) -> Result<Mmap> {
        self.inner.make_read_only()?;
        Ok(Mmap { inner: self.inner })
    }

    /// Transition the memory map to be readable and executable.
    ///
    /// If the memory map is file-backed, the file must have been opened with execute permissions.
    ///
    /// # Errors
    ///
    /// This method returns an error when the underlying system call fails, which can happen for a
    /// variety of reasons, such as when the file has not been opened with execute permissions.
    pub fn make_exec(mut self) -> Result<Mmap> {
        self.inner.make_exec()?;
        Ok(Mmap { inner: self.inner })
    }

    /// Uses `mlock` to lock the whole memory map into RAM.
    ///
    /// Note this requires privileged access.
    #[cfg(unix)]
    pub fn mlock(&mut self) -> Result<()> {
        self.inner.mlock()?;
        
        Ok(())
    }

    /// Uses `munlock` to unlock the whole memory map.
    ///
    /// Note this requires privileged access.
    #[cfg(unix)]
    pub fn munlock(&mut self) -> Result<()> {
        self.inner.munlock()?;
        
        Ok(())
    }
}

impl Deref for MmapMut {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.inner.ptr(), self.inner.len()) }
    }
}

impl DerefMut for MmapMut {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.inner.mut_ptr(), self.inner.len()) }
    }
}

impl AsRef<[u8]> for MmapMut {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl AsMut<[u8]> for MmapMut {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.deref_mut()
    }
}

impl fmt::Debug for MmapMut {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("MmapMut")
            .field("ptr", &self.as_ptr())
            .field("len", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod test {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::Arc;
    use std::thread;

    #[cfg(windows)]
    use winapi::um::winnt::GENERIC_ALL;

    use super::{Mmap, MmapMut, MmapOptions};

    #[test]
    fn map_file() {
        let expected_len = 128;
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();

        file.set_len(expected_len as u64).unwrap();

        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        let len = mmap.len();
        assert_eq!(expected_len, len);

        let zeros = vec![0; len];
        let incr: Vec<u8> = (0..len as u8).collect();

        // check that the mmap is empty
        assert_eq!(&zeros[..], &mmap[..]);

        // write values into the mmap
        (&mut mmap[..]).write_all(&incr[..]).unwrap();

        // read values back
        assert_eq!(&incr[..], &mmap[..]);
    }

    /// Checks that a 0-length file will not be mapped.
    #[test]
    fn map_empty_file() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        let mmap = unsafe { Mmap::map(&file) };
        assert!(mmap.is_err());
    }

    #[test]
    fn map_anon() {
        let expected_len = 128;
        let mut mmap = MmapMut::map_anon(expected_len).unwrap();
        let len = mmap.len();
        assert_eq!(expected_len, len);

        let zeros = vec![0; len];
        let incr: Vec<u8> = (0..len as u8).collect();

        // check that the mmap is empty
        assert_eq!(&zeros[..], &mmap[..]);

        // write values into the mmap
        (&mut mmap[..]).write_all(&incr[..]).unwrap();

        // read values back
        assert_eq!(&incr[..], &mmap[..]);
    }

    #[test]
    fn map_anon_zero_len() {
        assert!(MmapOptions::new().map_anon().is_err())
    }

    #[test]
    fn file_write() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        file.set_len(128).unwrap();

        let write = b"abc123";
        let mut read = [0u8; 6];

        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        (&mut mmap[..]).write_all(write).unwrap();
        mmap.flush().unwrap();

        file.read(&mut read).unwrap();
        assert_eq!(write, &read);
    }

    #[test]
    fn flush_range() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        file.set_len(128).unwrap();
        let write = b"abc123";

        let mut mmap = unsafe {
            MmapOptions::new()
                .offset(2)
                .len(write.len())
                .map_mut(&file)
                .unwrap()
        };
        (&mut mmap[..]).write_all(write).unwrap();
        mmap.flush_range(0, write.len()).unwrap();
    }

    #[test]
    fn map_copy() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        file.set_len(128).unwrap();

        let nulls = b"\0\0\0\0\0\0";
        let write = b"abc123";
        let mut read = [0u8; 6];

        let mut mmap = unsafe { MmapOptions::new().map_copy(&file).unwrap() };

        (&mut mmap[..]).write(write).unwrap();
        mmap.flush().unwrap();

        // The mmap contains the write
        (&mmap[..]).read(&mut read).unwrap();
        assert_eq!(write, &read);

        // The file does not contain the write
        file.read(&mut read).unwrap();
        assert_eq!(nulls, &read);

        // another mmap does not contain the write
        let mmap2 = unsafe { MmapOptions::new().map(&file).unwrap() };
        (&mmap2[..]).read(&mut read).unwrap();
        assert_eq!(nulls, &read);
    }

    #[test]
    fn map_offset() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();

        let offset = u32::max_value() as u64 + 2;
        let len = 5432;
        file.set_len(offset + len as u64).unwrap();

        // Check inferred length mmap.
        let mmap = unsafe { MmapOptions::new().offset(offset).map_mut(&file).unwrap() };
        assert_eq!(len, mmap.len());

        // Check explicit length mmap.
        let mut mmap = unsafe {
            MmapOptions::new()
                .offset(offset)
                .len(len)
                .map_mut(&file)
                .unwrap()
        };
        assert_eq!(len, mmap.len());

        let zeros = vec![0; len];
        let incr: Vec<_> = (0..len).map(|i| i as u8).collect();

        // check that the mmap is empty
        assert_eq!(&zeros[..], &mmap[..]);

        // write values into the mmap
        (&mut mmap[..]).write_all(&incr[..]).unwrap();

        // read values back
        assert_eq!(&incr[..], &mmap[..]);
    }

    #[test]
    fn index() {
        let mut mmap = MmapMut::map_anon(128).unwrap();
        mmap[0] = 42;
        assert_eq!(42, mmap[0]);
    }

    #[test]
    fn sync_send() {
        let mmap = Arc::new(MmapMut::map_anon(129).unwrap());
        thread::spawn(move || {
            &mmap[..];
        });
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn jit_x86(mut mmap: MmapMut) {
        use std::mem;
        mmap[0] = 0xB8; // mov eax, 0xAB
        mmap[1] = 0xAB;
        mmap[2] = 0x00;
        mmap[3] = 0x00;
        mmap[4] = 0x00;
        mmap[5] = 0xC3; // ret

        let mmap = mmap.make_exec().expect("make_exec");

        let jitfn: extern "C" fn() -> u8 = unsafe { mem::transmute(mmap.as_ptr()) };
        assert_eq!(jitfn(), 0xab);
    }

    #[test]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn jit_x86_anon() {
        jit_x86(MmapMut::map_anon(4096).unwrap());
    }

    #[test]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn jit_x86_file() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let mut options = OpenOptions::new();
        #[cfg(windows)]
        options.access_mode(GENERIC_ALL);

        let file = options
            .read(true)
            .write(true)
            .create(true)
            .open(&tempdir.path().join("jit_x86"))
            .expect("open");

        file.set_len(4096).expect("set_len");
        jit_x86(unsafe { MmapMut::map_mut(&file).expect("map_mut") });
    }

    #[test]
    fn mprotect_file() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let mut options = OpenOptions::new();
        #[cfg(windows)]
        options.access_mode(GENERIC_ALL);

        let mut file = options
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .expect("open");
        file.set_len(256 as u64).expect("set_len");

        let mmap = unsafe { MmapMut::map_mut(&file).expect("map_mut") };

        let mmap = mmap.make_read_only().expect("make_read_only");
        let mut mmap = mmap.make_mut().expect("make_mut");

        let write = b"abc123";
        let mut read = [0u8; 6];

        (&mut mmap[..]).write(write).unwrap();
        mmap.flush().unwrap();

        // The mmap contains the write
        (&mmap[..]).read(&mut read).unwrap();
        assert_eq!(write, &read);

        // The file should contain the write
        file.read(&mut read).unwrap();
        assert_eq!(write, &read);

        // another mmap should contain the write
        let mmap2 = unsafe { MmapOptions::new().map(&file).unwrap() };
        (&mmap2[..]).read(&mut read).unwrap();
        assert_eq!(write, &read);

        let mmap = mmap.make_exec().expect("make_exec");

        drop(mmap);
    }

    #[test]
    fn mprotect_copy() {
        let tempdir = tempdir::TempDir::new("mmap").unwrap();
        let path = tempdir.path().join("mmap");

        let mut options = OpenOptions::new();
        #[cfg(windows)]
        options.access_mode(GENERIC_ALL);

        let mut file = options
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .expect("open");
        file.set_len(256 as u64).expect("set_len");

        let mmap = unsafe { MmapOptions::new().map_copy(&file).expect("map_mut") };

        let mmap = mmap.make_read_only().expect("make_read_only");
        let mut mmap = mmap.make_mut().expect("make_mut");

        let nulls = b"\0\0\0\0\0\0";
        let write = b"abc123";
        let mut read = [0u8; 6];

        (&mut mmap[..]).write(write).unwrap();
        mmap.flush().unwrap();

        // The mmap contains the write
        (&mmap[..]).read(&mut read).unwrap();
        assert_eq!(write, &read);

        // The file does not contain the write
        file.read(&mut read).unwrap();
        assert_eq!(nulls, &read);

        // another mmap does not contain the write
        let mmap2 = unsafe { MmapOptions::new().map(&file).unwrap() };
        (&mmap2[..]).read(&mut read).unwrap();
        assert_eq!(nulls, &read);

        let mmap = mmap.make_exec().expect("make_exec");

        drop(mmap);
    }

    #[test]
    fn mprotect_anon() {
        let mmap = MmapMut::map_anon(256).expect("map_mut");

        let mmap = mmap.make_read_only().expect("make_read_only");
        let mmap = mmap.make_mut().expect("make_mut");
        let mmap = mmap.make_exec().expect("make_exec");
        drop(mmap);
    }
}
