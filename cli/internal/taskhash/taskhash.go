// package taskhash handles calculating dependency hashes for nodes in the task execution
// graph.

package taskhash

import (
	"fmt"
	"os"
	"sort"
	"strings"
	"sync"

	"github.com/pyr-sh/dag"
	gitignore "github.com/sabhiram/go-gitignore"
	"github.com/vercel/turborepo/cli/internal/doublestar"
	"github.com/vercel/turborepo/cli/internal/fs"
	"github.com/vercel/turborepo/cli/internal/nodes"
	"github.com/vercel/turborepo/cli/internal/turbopath"
	"github.com/vercel/turborepo/cli/internal/util"
	"golang.org/x/sync/errgroup"
)

// Tracker caches package-inputs hashes, as well as package-task hashes.
// package-inputs hashes must be calculated before package-task hashes,
// and package-task hashes must be calculated in topographical order.
// package-task hashing is threadsafe, provided topographical order is
// respected.
type Tracker struct {
	rootNode            string
	globalHash          string
	pipeline            fs.Pipeline
	packageInfos        map[interface{}]*fs.PackageJSON
	mu                  sync.RWMutex
	packageInputsHashes packageFileHashes
	packageTaskHashes   map[string]string // taskID -> hash
}

// NewTracker creates a tracker for package-inputs combinations and package-task combinations.
func NewTracker(rootNode string, globalHash string, pipeline fs.Pipeline, packageInfos map[interface{}]*fs.PackageJSON) *Tracker {
	return &Tracker{
		rootNode:          rootNode,
		globalHash:        globalHash,
		pipeline:          pipeline,
		packageInfos:      packageInfos,
		packageTaskHashes: make(map[string]string),
	}
}

// packageFileSpec defines a combination of a package and optional set of input globs
type packageFileSpec struct {
	pkg    string
	inputs []string
}

func specFromPackageTask(pt *nodes.PackageTask) packageFileSpec {
	return packageFileSpec{
		pkg:    pt.PackageName,
		inputs: pt.TaskDefinition.Inputs,
	}
}

// packageFileHashKey is a hashable representation of a packageFileSpec.
type packageFileHashKey string

func (pfs packageFileSpec) ToKey() packageFileHashKey {
	sort.Strings(pfs.inputs)
	return packageFileHashKey(fmt.Sprintf("%v#%v", pfs.pkg, strings.Join(pfs.inputs, "!")))
}

func safeCompileIgnoreFile(filepath string) (*gitignore.GitIgnore, error) {
	if fs.FileExists(filepath) {
		return gitignore.CompileIgnoreFile(filepath)
	}
	// no op
	return gitignore.CompileIgnoreLines([]string{}...), nil
}

func (pfs *packageFileSpec) hash(pkg *fs.PackageJSON, repoRoot fs.AbsolutePath) (string, error) {
	hashObject, pkgDepsErr := fs.GetPackageDeps(repoRoot, &fs.PackageDepsOptions{
		PackagePath:   pkg.Dir,
		InputPatterns: pfs.inputs,
	})
	if pkgDepsErr != nil {
		manualHashObject, err := manuallyHashPackage(pkg, pfs.inputs, repoRoot)
		if err != nil {
			return "", err
		}
		hashObject = manualHashObject
	}
	hashOfFiles, otherErr := fs.HashObject(hashObject)
	if otherErr != nil {
		return "", otherErr
	}
	return hashOfFiles, nil
}

func manuallyHashPackage(pkg *fs.PackageJSON, inputs []string, rootPath fs.AbsolutePath) (map[turbopath.AnchoredUnixPath]string, error) {
	hashObject := make(map[turbopath.AnchoredUnixPath]string)
	// Instead of implementing all gitignore properly, we hack it. We only respect .gitignore in the root and in
	// the directory of a package.
	ignore, err := safeCompileIgnoreFile(rootPath.Join(".gitignore").ToString())
	if err != nil {
		return nil, err
	}

	ignorePkg, err := safeCompileIgnoreFile(rootPath.Join(pkg.Dir, ".gitignore").ToString())
	if err != nil {
		return nil, err
	}

	includePattern := ""
	if len(inputs) > 0 {
		includePattern = "{" + strings.Join(inputs, ",") + "}"
	}

	pathPrefix := rootPath.Join(pkg.Dir).ToString()
	convertedPathPrefix := turbopath.AbsoluteSystemPath(pathPrefix)
	fs.Walk(pathPrefix, func(name string, isDir bool) error {
		convertedName := turbopath.AbsoluteSystemPath(name)
		rootMatch := ignore.MatchesPath(convertedName.ToString())
		otherMatch := ignorePkg.MatchesPath(convertedName.ToString())
		if !rootMatch && !otherMatch {
			if !isDir {
				if includePattern != "" {
					val, err := doublestar.PathMatch(includePattern, convertedName.ToString())
					if err != nil {
						return err
					}
					if !val {
						return nil
					}
				}
				hash, err := fs.GitLikeHashFile(convertedName.ToString())
				if err != nil {
					return fmt.Errorf("could not hash file %v. \n%w", convertedName.ToString(), err)
				}

				relativePath, err := convertedName.RelativeTo(convertedPathPrefix)
				if err != nil {
					return fmt.Errorf("File path cannot be made relative: %w", err)
				}
				hashObject[relativePath.ToUnixPath()] = hash
			}
		}
		return nil
	})
	return hashObject, nil
}

// packageFileHashes is a map from a package and optional input globs to the hash of
// the matched files in the package.
type packageFileHashes map[packageFileHashKey]string

// CalculateFileHashes hashes each unique package-inputs combination that is present
// in the task graph. Must be called before calculating task hashes.
func (th *Tracker) CalculateFileHashes(allTasks []dag.Vertex, workerCount int, repoRoot fs.AbsolutePath) error {
	hashTasks := make(util.Set)
	for _, v := range allTasks {
		taskID, ok := v.(string)
		if !ok {
			return fmt.Errorf("unknown task %v", taskID)
		}
		if taskID == th.rootNode {
			continue
		}
		pkgName, _ := util.GetPackageTaskFromId(taskID)
		if pkgName == th.rootNode {
			continue
		}
		taskDefinition, ok := th.pipeline.GetTaskDefinition(taskID)
		if !ok {
			return fmt.Errorf("missing pipeline entry %v", taskID)
		}
		hashTasks.Add(&packageFileSpec{
			pkg:    pkgName,
			inputs: taskDefinition.Inputs,
		})
	}

	hashes := make(map[packageFileHashKey]string)
	hashQueue := make(chan *packageFileSpec, workerCount)
	hashErrs := &errgroup.Group{}
	for i := 0; i < workerCount; i++ {
		hashErrs.Go(func() error {
			for ht := range hashQueue {
				pkg, ok := th.packageInfos[ht.pkg]
				if !ok {
					return fmt.Errorf("cannot find package %v", ht.pkg)
				}
				hash, err := ht.hash(pkg, repoRoot)
				if err != nil {
					return err
				}
				th.mu.Lock()
				hashes[ht.ToKey()] = hash
				th.mu.Unlock()
			}
			return nil
		})
	}
	for ht := range hashTasks {
		hashQueue <- ht.(*packageFileSpec)
	}
	close(hashQueue)
	err := hashErrs.Wait()
	if err != nil {
		return err
	}
	th.packageInputsHashes = hashes
	return nil
}

type taskHashInputs struct {
	hashOfFiles          string
	externalDepsHash     string
	task                 string
	outputs              []string
	passThruArgs         []string
	hashableEnvPairs     []string
	globalHash           string
	taskDependencyHashes []string
}

func (th *Tracker) calculateDependencyHashes(dependencySet dag.Set) ([]string, error) {
	dependencyHashSet := make(util.Set)

	rootPrefix := th.rootNode + util.TaskDelimiter
	th.mu.RLock()
	defer th.mu.RUnlock()
	for _, dependency := range dependencySet {
		if dependency == th.rootNode {
			continue
		}
		dependencyTask, ok := dependency.(string)
		if !ok {
			return nil, fmt.Errorf("unknown task: %v", dependency)
		}
		if strings.HasPrefix(dependencyTask, rootPrefix) {
			continue
		}
		dependencyHash, ok := th.packageTaskHashes[dependencyTask]
		if !ok {
			return nil, fmt.Errorf("missing hash for dependent task: %v", dependencyTask)
		}
		dependencyHashSet.Add(dependencyHash)
	}
	dependenciesHashList := dependencyHashSet.UnsafeListOfStrings()
	sort.Strings(dependenciesHashList)
	return dependenciesHashList, nil
}

// CalculateTaskHash calculates the hash for package-task combination. It is threadsafe, provided
// that it has previously been called on its task-graph dependencies. File hashes must be calculated
// first.
func (th *Tracker) CalculateTaskHash(pt *nodes.PackageTask, dependencySet dag.Set, args []string) (string, error) {
	pkgFileHashKey := specFromPackageTask(pt).ToKey()
	hashOfFiles, ok := th.packageInputsHashes[pkgFileHashKey]
	if !ok {
		return "", fmt.Errorf("cannot find package-file hash for %v", pkgFileHashKey)
	}
	outputs := pt.HashableOutputs()
	hashableEnvPairs := []string{}
	for _, envVar := range pt.TaskDefinition.EnvVarDependencies {
		hashableEnvPairs = append(hashableEnvPairs, fmt.Sprintf("%v=%v", envVar, os.Getenv(envVar)))
	}
	sort.Strings(hashableEnvPairs)
	taskDependencyHashes, err := th.calculateDependencyHashes(dependencySet)
	if err != nil {
		return "", err
	}
	hash, err := fs.HashObject(&taskHashInputs{
		hashOfFiles:          hashOfFiles,
		externalDepsHash:     pt.Pkg.ExternalDepsHash,
		task:                 pt.Task,
		outputs:              outputs,
		passThruArgs:         args,
		hashableEnvPairs:     hashableEnvPairs,
		globalHash:           th.globalHash,
		taskDependencyHashes: taskDependencyHashes,
	})
	if err != nil {
		return "", fmt.Errorf("failed to hash task %v: %v", pt.TaskID, hash)
	}
	th.mu.Lock()
	th.packageTaskHashes[pt.TaskID] = hash
	th.mu.Unlock()
	return hash, nil
}
