package cacheitem

import (
	"archive/tar"
	"compress/gzip"
	"errors"
	"io"
	"os"
	"runtime"
	"strings"

	"github.com/vercel/turborepo/cli/internal/turbopath"
)

// Open returns an existing CacheItem at the specified path.
func Open(path turbopath.AbsoluteSystemPath) (*CacheItem, error) {
	handle, err := os.Open(path.ToString())
	if err != nil {
		return nil, err
	}

	return &CacheItem{
		Path:   path,
		handle: handle,
	}, nil
}

// Restore extracts a cache to a specified disk location.
func (ci *CacheItem) Restore(anchor turbopath.AbsoluteSystemPath) ([]turbopath.AnchoredSystemPath, error) {
	// tar wrapped in gzip, we need to stream out of gzip first.
	gzr, err := gzip.NewReader(ci.handle)
	if err != nil {
		return nil, err
	}
	defer func() { _ = gzr.Close() }()
	tr := tar.NewReader(gzr)

	// On first attempt to restore it's possible that a link target doesn't exist.
	// Save them and topsort them.
	var symlinks []*tar.Header

	restored := make([]turbopath.AnchoredSystemPath, 0)
	for {
		header, trErr := tr.Next()
		if trErr == io.EOF {
			// The end, time to restore any missing links.
			symlinksRestored, symlinksErr := topologicallyRestoreSymlinks(anchor, symlinks, tr)
			restored = append(restored, symlinksRestored...)
			if symlinksErr != nil {
				return restored, symlinksErr
			}

			break
		}
		if trErr != nil {
			return restored, trErr
		}

		// The reader will not advance until tr.Next is called.
		// We can treat this as file metadata + body reader.

		// Attempt to place the file on disk.
		file, restoreErr := restoreEntry(anchor, header, tr)
		if restoreErr != nil {
			if errors.Is(restoreErr, errMissingSymlinkTarget) {
				// Links get one shot to be valid, then they're accumulated, DAG'd, and restored on delay.
				symlinks = append(symlinks, header)
				continue
			}
			return restored, restoreErr
		}
		restored = append(restored, file)
	}

	return restored, nil
}

// restoreRegular is the entry point for all things read from the tar.
func restoreEntry(anchor turbopath.AbsoluteSystemPath, header *tar.Header, reader *tar.Reader) (turbopath.AnchoredSystemPath, error) {
	// We're permissive on creation, but restrictive on restoration.
	// There is no need to prevent the cache creation in any case.
	// And on restoration, if we fail, we simply run the task.
	switch header.Typeflag {
	case tar.TypeDir:
		return restoreDirectory(anchor, header, reader)
	case tar.TypeReg:
		return restoreRegular(anchor, header, reader)
	case tar.TypeSymlink:
		return restoreSymlink(anchor, header, reader)
	default:
		return "", errUnsupportedFileType
	}
}

// canonicalizeName returns either an AnchoredSystemPath or an error.
func canonicalizeName(name string) (turbopath.AnchoredSystemPath, error) {
	// Assuming this was a `turbo`-created input, we currently have an AnchoredUnixPath.
	// Assuming this is malicious input we don't really care if we do the wrong thing.
	wellFormed, windowsSafe := checkName(name)

	// Determine if the future filename is a well-formed AnchoredUnixPath
	if !wellFormed {
		return "", errNameMalformed
	}

	// Determine if the AnchoredUnixPath is safe to be used on Windows
	if runtime.GOOS == "windows" && !windowsSafe {
		return "", errNameWindowsUnsafe
	}

	// Okay, we're all set here.
	return turbopath.AnchoredUnixPath(name).ToSystemPath(), nil
}

// checkName returns `wellFormed, windowsSafe` via inspection of separators and traversal
func checkName(name string) (bool, bool) {
	length := len(name)

	// Name is of length 0.
	if length == 0 {
		return false, false
	}

	wellFormed := true
	windowsSafe := true

	// Name starts with:
	// - `/`
	// - `./`
	// - `../`
	if strings.HasPrefix(name, "/") || strings.HasPrefix(name, "./") || strings.HasPrefix(name, "../") {
		wellFormed = false
	}

	// Name ends in:
	// - `/.`
	// - `/..`
	if strings.HasSuffix(name, "/.") || strings.HasSuffix(name, "/..") {
		wellFormed = false
	}

	// Name contains:
	// - `//`
	// - `/./`
	// - `/../`
	if strings.Contains(name, "//") || strings.Contains(name, "/./") || strings.Contains(name, "/../") {
		wellFormed = false
	}

	// Name contains: `\`
	if strings.ContainsRune(name, '\\') {
		windowsSafe = false
	}

	return wellFormed, windowsSafe
}
