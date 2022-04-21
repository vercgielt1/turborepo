package globby

import (
	"path/filepath"
	"reflect"
	"testing"

	"github.com/spf13/afero"
)

func setup(files []string) func() {
	var original = getFileSystem()
	var fs = afero.NewMemMapFs()

	setFileSystem(fs)

	for _, file := range files {
		// We don't need the handle, we don't need the error.
		// We'll know if it errors because the tests will not pass.
		// nolint:errcheck
		fs.Create(file)
	}

	return func() {
		setFileSystem(original)
	}
}

func TestGlobFiles(t *testing.T) {
	type args struct {
		basePath        string
		includePatterns []string
		excludePatterns []string
	}
	tests := []struct {
		name  string
		files []string
		args  args
		want  []string
	}{
		{
			name:  "hello world",
			files: []string{"/test.txt"},
			args: args{
				basePath:        "/",
				includePatterns: []string{"*.txt"},
				excludePatterns: []string{},
			},
			want: []string{"/test.txt"},
		},
		{
			name: "finding workspace package.json files",
			files: []string{
				"/external/file.txt",
				"/repos/some-app/apps/docs/package.json",
				"/repos/some-app/apps/web/package.json",
				"/repos/some-app/bower_components/readline/package.json",
				"/repos/some-app/examples/package.json",
				"/repos/some-app/node_modules/gulp/bower_components/readline/package.json",
				"/repos/some-app/node_modules/react/package.json",
				"/repos/some-app/package.json",
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
				"/repos/some-app/test/mocks/kitchen-sink/package.json",
				"/repos/some-app/tests/mocks/kitchen-sink/package.json",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"packages/*/package.json", "apps/*/package.json"},
				excludePatterns: []string{"**/node_modules/", "**/bower_components/", "**/test/", "**/tests/"},
			},
			want: []string{
				"/repos/some-app/apps/docs/package.json",
				"/repos/some-app/apps/web/package.json",
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
			},
		},
		{
			name: "excludes unexpected workspace package.json files",
			files: []string{
				"/external/file.txt",
				"/repos/some-app/apps/docs/package.json",
				"/repos/some-app/apps/web/package.json",
				"/repos/some-app/bower_components/readline/package.json",
				"/repos/some-app/examples/package.json",
				"/repos/some-app/node_modules/gulp/bower_components/readline/package.json",
				"/repos/some-app/node_modules/react/package.json",
				"/repos/some-app/package.json",
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
				"/repos/some-app/test/mocks/spanish-inquisition/package.json",
				"/repos/some-app/tests/mocks/spanish-inquisition/package.json",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**/package.json"},
				excludePatterns: []string{"**/node_modules/", "**/bower_components/", "**/test/", "**/tests/"},
			},
			want: []string{
				"/repos/some-app/apps/docs/package.json",
				"/repos/some-app/apps/web/package.json",
				"/repos/some-app/examples/package.json",
				"/repos/some-app/package.json",
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
			},
		},
		{
			name: "nested packages work",
			files: []string{
				"/external/file.txt",
				"/repos/some-app/apps/docs/package.json",
				"/repos/some-app/apps/web/package.json",
				"/repos/some-app/bower_components/readline/package.json",
				"/repos/some-app/examples/package.json",
				"/repos/some-app/node_modules/gulp/bower_components/readline/package.json",
				"/repos/some-app/node_modules/react/package.json",
				"/repos/some-app/package.json",
				"/repos/some-app/packages/xzibit/package.json",
				"/repos/some-app/packages/xzibit/node_modules/street-legal/package.json",
				"/repos/some-app/packages/xzibit/node_modules/paint-colors/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/node_modules/meme/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/node_modules/yo-dawg/package.json",
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
				"/repos/some-app/test/mocks/spanish-inquisition/package.json",
				"/repos/some-app/tests/mocks/spanish-inquisition/package.json",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"packages/**/package.json"},
				excludePatterns: []string{"**/node_modules/", "**/bower_components/", "**/test/", "**/tests/"},
			},
			want: []string{
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
				"/repos/some-app/packages/xzibit/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/package.json",
			},
		},
		{
			name: "includes do not override excludes",
			files: []string{
				"/external/file.txt",
				"/repos/some-app/apps/docs/package.json",
				"/repos/some-app/apps/web/package.json",
				"/repos/some-app/bower_components/readline/package.json",
				"/repos/some-app/examples/package.json",
				"/repos/some-app/node_modules/gulp/bower_components/readline/package.json",
				"/repos/some-app/node_modules/react/package.json",
				"/repos/some-app/package.json",
				"/repos/some-app/packages/xzibit/package.json",
				"/repos/some-app/packages/xzibit/node_modules/street-legal/package.json",
				"/repos/some-app/packages/xzibit/node_modules/paint-colors/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/node_modules/meme/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/node_modules/yo-dawg/package.json",
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
				"/repos/some-app/test/mocks/spanish-inquisition/package.json",
				"/repos/some-app/tests/mocks/spanish-inquisition/package.json",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"packages/**/package.json", "tests/mocks/*/package.json"},
				excludePatterns: []string{"**/node_modules/", "**/bower_components/", "**/test/", "**/tests/"},
			},
			want: []string{
				"/repos/some-app/packages/colors/package.json",
				"/repos/some-app/packages/faker/package.json",
				"/repos/some-app/packages/left-pad/package.json",
				"/repos/some-app/packages/xzibit/package.json",
				"/repos/some-app/packages/xzibit/packages/yo-dawg/package.json",
			},
		},
		{
			name: "output globbing grabs the desired content",
			files: []string{
				"/external/file.txt",
				"/repos/some-app/src/index.js",
				"/repos/some-app/public/src/css/index.css",
				"/repos/some-app/.turbo/turbo-build.log",
				"/repos/some-app/.turbo/somebody-touched-this-file-into-existence.txt",
				"/repos/some-app/.next/log.txt",
				"/repos/some-app/.next/cache/db6a76a62043520e7aaadd0bb2104e78.txt",
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
				"/repos/some-app/public/dist/css/index.css",
				"/repos/some-app/public/dist/images/rick_astley.jpg",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{".turbo/turbo-build.log", "dist/**", ".next/**", "public/dist/**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/.next/cache/db6a76a62043520e7aaadd0bb2104e78.txt",
				"/repos/some-app/.next/log.txt",
				"/repos/some-app/.turbo/turbo-build.log",
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
				"/repos/some-app/public/dist/css/index.css",
				"/repos/some-app/public/dist/images/rick_astley.jpg",
			},
		},
		{
			name: "passing ** captures all children",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"dist/**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "passing just a directory captures no children",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"dist"},
				excludePatterns: []string{},
			},
			want: []string{},
		},
		{
			name: "redundant includes do not duplicate",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**/*", "dist/**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "exclude everything, include everything",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**"},
				excludePatterns: []string{"**"},
			},
			want: []string{},
		},
		{
			name: "passing just a directory to exclude prevents capture of children",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"dist/**"},
				excludePatterns: []string{"dist/js"},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
			},
		},
		{
			name: "passing ** to exclude prevents capture of children",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"dist/**"},
				excludePatterns: []string{"dist/js/**"},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
			},
		},
		{
			name: "exclude everything with folder . does not apply at base path",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**"},
				excludePatterns: []string{"./"},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "exclude everything with traversal applies at a non-base path",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**"},
				excludePatterns: []string{"./dist"},
			},
			want: []string{},
		},
		{
			name: "exclude everything with folder traversal (..) does not apply at base path",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**"},
				excludePatterns: []string{"dist/../"},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "how do globs even work bad glob microformat",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**/**/**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "directory traversal stops at base path",
			files: []string{
				"/repos/spanish-inquisition/index.html",
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"../spanish-inquisition/**", "dist/**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "globs and traversal and globs do not cross base path",
			files: []string{
				"/repos/spanish-inquisition/index.html",
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**/../../spanish-inquisition/**"},
				excludePatterns: []string{},
			},
			want: []string{},
		},
		{
			name: "traversal works within base path",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"dist/js/../**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "self-references (.) work",
			files: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"dist/./././**"},
				excludePatterns: []string{},
			},
			want: []string{
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
		},
		{
			name: "depth of 1 includes does not capture folders",
			files: []string{
				"/repos/some-app/package.json",
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"*"},
				excludePatterns: []string{},
			},
			want: []string{"/repos/some-app/package.json"},
		},
		{
			name: "depth of 1 excludes prevents capturing folders",
			files: []string{
				"/repos/some-app/package.json",
				"/repos/some-app/dist/index.html",
				"/repos/some-app/dist/js/index.js",
				"/repos/some-app/dist/js/lib.js",
				"/repos/some-app/dist/js/node_modules/browserify.js",
			},
			args: args{
				basePath:        "/repos/some-app/",
				includePatterns: []string{"**"},
				excludePatterns: []string{"dist/*"},
			},
			want: []string{"/repos/some-app/package.json"},
		},
	}
	for _, tt := range tests {
		t.Cleanup(setup(tt.files))
		t.Run(tt.name, func(t *testing.T) {
			got := GlobFiles(tt.args.basePath, tt.args.includePatterns, tt.args.excludePatterns)

			var gotToSlash = make([]string, len(got))
			for index, path := range got {
				gotToSlash[index] = filepath.ToSlash(path)
			}

			// If the length of both are zero, we're already good to go.
			if len(got) != 0 || len(tt.want) != 0 {
				if !reflect.DeepEqual(gotToSlash, tt.want) {
					t.Errorf("GlobFiles() = %v, want %v", gotToSlash, tt.want)
				}
			}
		})
	}
}
