package packagemanager

import (
	"fmt"

	"github.com/vercel/turborepo/cli/internal/fs"
	"github.com/vercel/turborepo/cli/internal/turbopath"
)

var nodejsNpm = PackageManager{
	Name:         "nodejs-npm",
	Slug:         "npm",
	Command:      "npm",
	Specfile:     "package.json",
	Lockfile:     "package-lock.json",
	PackageDir:   "node_modules",
	ArgSeparator: []string{"--"},

	getWorkspaceGlobs: func(rootpath turbopath.AbsolutePath) ([]string, error) {
		pkg, err := fs.ReadPackageJSON(rootpath.UnsafeJoin("package.json"))
		if err != nil {
			return nil, fmt.Errorf("package.json: %w", err)
		}
		if len(pkg.Workspaces) == 0 {
			return nil, fmt.Errorf("package.json: no workspaces found. Turborepo requires npm workspaces to be defined in the root package.json")
		}
		return pkg.Workspaces, nil
	},

	getWorkspaceIgnores: func(pm PackageManager, rootpath turbopath.AbsolutePath) ([]string, error) {
		// Matches upstream values:
		// function: https://github.com/npm/map-workspaces/blob/a46503543982cb35f51cc2d6253d4dcc6bca9b32/lib/index.js#L73
		// key code: https://github.com/npm/map-workspaces/blob/a46503543982cb35f51cc2d6253d4dcc6bca9b32/lib/index.js#L90-L96
		// call site: https://github.com/npm/cli/blob/7a858277171813b37d46a032e49db44c8624f78f/lib/workspaces/get-workspaces.js#L14
		return []string{
			"**/node_modules/**",
		}, nil
	},

	Matches: func(manager string, version string) (bool, error) {
		return manager == "npm", nil
	},

	detect: func(projectDirectory turbopath.AbsolutePath, packageManager *PackageManager) (bool, error) {
		specfileExists := projectDirectory.UnsafeJoin(packageManager.Specfile).FileExists()
		lockfileExists := projectDirectory.UnsafeJoin(packageManager.Lockfile).FileExists()

		return (specfileExists && lockfileExists), nil
	},
}
