package turbopath

import (
	"path"
	"path/filepath"
)

// AnchoredUnixPath is a path stemming from a specified root using Unix `/` separators.
type AnchoredUnixPath string

// For interface reasons, we need a way to distinguish between
// Absolute/Anchored/Relative/System/Unix/File paths so we stamp them.
func (AnchoredUnixPath) anchoredStamp() {}
func (AnchoredUnixPath) unixPathStamp() {}
func (AnchoredUnixPath) filePathStamp() {}

// ToString returns a string represenation of this Path.
// Used for interfacing with APIs that require a string.
func (p AnchoredUnixPath) ToString() string {
	return string(p)
}

// ToSystemPath converts a AnchoredUnixPath to a AnchoredSystemPath.
func (p AnchoredUnixPath) ToSystemPath() AnchoredSystemPath {
	return AnchoredSystemPath(filepath.FromSlash(p.ToString()))
}

// ToUnixPath returns itself.
func (p AnchoredUnixPath) ToUnixPath() AnchoredUnixPath {
	return p
}

// RelativeTo calculates the relative path between two `AnchoredUnixPath`s.
func (p AnchoredUnixPath) RelativeTo(basePath AnchoredUnixPath) (AnchoredUnixPath, error) {
	processed, err := filepath.Rel(basePath.ToString(), p.ToString())
	return AnchoredUnixPath(processed), err
}

// RestoreAnchor prefixes the AnchoredUnixPath with its anchor to return an AbsoluteUnixPath.
func (p AnchoredUnixPath) RestoreAnchor(anchor AbsoluteUnixPath) AbsoluteUnixPath {
	return AbsoluteUnixPath(filepath.Join(anchor.ToString(), p.ToString()))
}

// Join appends relative path segments to this RelativeUnixPath.
func (p AnchoredUnixPath) Join(additional ...RelativeUnixPath) AnchoredUnixPath {
	cast := RelativeUnixPathArray(additional)
	return AnchoredUnixPath(path.Join(p.ToString(), path.Join(cast.ToStringArray()...)))
}
