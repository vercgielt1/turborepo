// Package graph contains the CompleteGraph struct and some methods around it
package graph

import (
	gocontext "context"
	"errors"
	"fmt"
	"path/filepath"

	"github.com/pyr-sh/dag"
	"github.com/vercel/turbo/cli/internal/fs"
	"github.com/vercel/turbo/cli/internal/nodes"
	"github.com/vercel/turbo/cli/internal/turbopath"
	"github.com/vercel/turbo/cli/internal/util"
)

// WorkspaceInfos holds information about each workspace in the monorepo.
type WorkspaceInfos struct {
	PackageJSONs map[string]*fs.PackageJSON
	TurboConfigs map[string]*fs.TurboJSON
}

// CompleteGraph represents the common state inferred from the filesystem and pipeline.
// It is not intended to include information specific to a particular run.
type CompleteGraph struct {
	// WorkspaceGraph expresses the dependencies between packages
	WorkspaceGraph dag.AcyclicGraph

	// Pipeline is config from turbo.json
	Pipeline fs.Pipeline

	// WorkspaceInfos stores the package.json contents by package name
	WorkspaceInfos WorkspaceInfos

	// GlobalHash is the hash of all global dependencies
	GlobalHash string

	RootNode string

	// Map of TaskDefinitions by taskID
	TaskDefinitions map[string]*fs.TaskDefinition
	RepoRoot        turbopath.AbsoluteSystemPath
}

// TODO(mehulkar): make it possible to append info into this error
var TurboJSONValidationError = errors.New("turbo.json failed validation")

// GetPackageTaskVisitor wraps a `visitor` function that is used for walking the TaskGraph
// during execution (or dry-runs). The function returned here does not execute any tasks itself,
// but it helps curry some data from the Complete Graph and pass it into the visitor function.
func (g *CompleteGraph) GetPackageTaskVisitor(ctx gocontext.Context, visitor func(ctx gocontext.Context, packageTask *nodes.PackageTask) error) func(taskID string) error {
	return func(taskID string) error {
		packageName, taskName := util.GetPackageTaskFromId(taskID)
		pkg, ok := g.WorkspaceInfos.PackageJSONs[packageName]
		if !ok {
			return fmt.Errorf("cannot find package %v for task %v", packageName, taskID)
		}

		taskDefinition, ok := g.TaskDefinitions[taskID]
		if !ok {
			return fmt.Errorf("Could not find definition for task")
		}

		packageTask := &nodes.PackageTask{
			TaskID:          taskID,
			Task:            taskName,
			PackageName:     packageName,
			Pkg:             pkg,
			Dir:             pkg.Dir.ToString(),
			TaskDefinition:  taskDefinition,
			Outputs:         taskDefinition.Outputs.Inclusions,
			ExcludedOutputs: taskDefinition.Outputs.Exclusions,
		}

		if cmd, ok := pkg.Scripts[taskName]; ok {
			packageTask.Command = cmd
		}

		packageTask.LogFile = repoRelativeLogFile(packageTask)

		return visitor(ctx, packageTask)
	}
}

// GetPipelineFromWorkspace returns the Unmarshaled fs.Pipeline struct from turbo.json in the given workspace.
func (g *CompleteGraph) GetPipelineFromWorkspace(workspaceName string, isSinglePackage bool) (fs.Pipeline, error) {
	turboConfig, err := g.GetTurboConfigFromWorkspace(workspaceName, isSinglePackage)

	if err != nil {
		return nil, err
	}

	return turboConfig.Pipeline, nil
}

// GetTurboConfigFromWorkspace returns the Unmarshaled fs.TurboJSON from turbo.json in the given workspace.
func (g *CompleteGraph) GetTurboConfigFromWorkspace(workspaceName string, isSinglePackage bool) (*fs.TurboJSON, error) {
	cachedTurboConfig, ok := g.WorkspaceInfos.TurboConfigs[workspaceName]

	if ok {
		return cachedTurboConfig, nil
	}

	// Note: dir for the root workspace will be an empty string, and for
	// other workspaces, it will be a relative path.
	pkgJSON, err := g.GetPackageJSONFromWorkspace(workspaceName)
	if err != nil {
		return &fs.TurboJSON{}, nil
	}

	workspaceAbsolutePath := pkgJSON.Dir.RestoreAnchor(g.RepoRoot)
	turboConfig, err := fs.LoadTurboConfig(workspaceAbsolutePath, pkgJSON, isSinglePackage)

	// If we failed to load a TurboConfig, return the error
	if err != nil {
		return nil, err
	}

	if workspaceName != util.RootPkgName {
		// TODO(mehulkar) pass in validation functions and group errors
		if err := validateTurboConfig(turboConfig, workspaceName); err != nil {
			return nil, TurboJSONValidationError
		}
	}

	// add to cache
	g.WorkspaceInfos.TurboConfigs[workspaceName] = turboConfig

	return g.WorkspaceInfos.TurboConfigs[workspaceName], nil
}

// GetPackageJSONFromWorkspace returns an Unmarshaled struct of the package.json in the given workspace
func (g *CompleteGraph) GetPackageJSONFromWorkspace(workspaceName string) (*fs.PackageJSON, error) {
	pkgJSON, ok := g.WorkspaceInfos.PackageJSONs[workspaceName]

	if !ok {
		return &fs.PackageJSON{}, nil
	}

	return pkgJSON, nil
}

// repoRelativeLogFile returns the path to the log file for this task execution as a
// relative path from the root of the monorepo.
func repoRelativeLogFile(pt *nodes.PackageTask) string {
	return filepath.Join(pt.Pkg.Dir.ToStringDuringMigration(), ".turbo", fmt.Sprintf("turbo-%v.log", pt.Task))
}

func validateTurboConfig(turboConfig *fs.TurboJSON, workspaceName string) error {
	for taskIdOrName, _ := range turboConfig.Pipeline {
		if util.IsPackageTask(taskIdOrName) {
			taskName := util.StripPackageName(taskIdOrName)
			return fmt.Errorf("xxDetected %s in %s, use %s instead", taskIdOrName, workspaceName, taskName)
		}
	}

	return nil
}
