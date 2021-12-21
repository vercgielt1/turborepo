import execa from "execa";
import fs from "fs-extra";
import os from "os";
import path from "path";
const isWin = process.platform === "win32";
const turboPath = path.join(__dirname, "../turbo" + (isWin ? ".exe" : ""));

type NpmClient = "npm" | "pnpm" | "yarn";

export class Monorepo {
  static tmpdir = os.tmpdir();
  static yarnCache = path.join(__dirname, "yarn-cache-");
  root: string;
  subdir?: string;
  name: string;
  npmClient: NpmClient;
  get nodeModulesPath() {
    return this.subdir
      ? path.join(this.root, this.subdir, "node_modules")
      : path.join(this.root, "node_modules");
  }

  constructor(name) {
    this.root = fs.mkdtempSync(path.join(__dirname, `turbo-monorepo-${name}-`));
  }

  init(npmClient: NpmClient, turboConfig = {}, subdir?: string) {
    this.npmClient = npmClient;
    this.subdir = subdir;
    fs.removeSync(path.join(this.root, ".git"));
    fs.ensureDirSync(path.join(this.root, ".git"));
    if (this.subdir) {
      fs.ensureDirSync(path.join(this.root, this.subdir));
    }
    fs.writeFileSync(
      path.join(this.root, ".git", "config"),
      `
  [user]
    name = GitHub Actions
    email = actions@users.noreply.github.com
  
  [init]
   defaultBranch = main
  `
    );
    execa.sync("git", ["init", "-q"], { cwd: this.root });
    this.generateRepoFiles(turboConfig);
  }

  install() {
    if (!fs.existsSync(this.nodeModulesPath)) {
      fs.mkdirSync(this.nodeModulesPath, { recursive: true });
    }
  }

  /**
   * Simulates a "yarn" call by linking internal packages and generates a yarn.lock file
   */
  linkPackages() {
    const cwd = this.subdir ? path.join(this.root, this.subdir) : this.root;
    const pkgs = fs.readdirSync(path.join(cwd, "packages"));

    if (!fs.existsSync(this.nodeModulesPath)) {
      fs.mkdirSync(this.nodeModulesPath, { recursive: true });
    }

    let yarnYaml = `# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.\n# yarn lockfile v1\n`;

    if (this.npmClient == "pnpm") {
      this.commitFiles({
        "pnpm-workspace.yaml": `packages:
- packages/*`,
        "pnpm-lock.yaml": `lockfileVersion: 5.3
        
importers:

  .:
    specifiers: {}

  packages/a:
    specifiers:
      b: workspace:*
    dependencies:
      b: link:../b

  packages/b:
    specifiers: {}

  packages/c:
    specifiers: {}
        
        `,
      });
      execa.sync("pnpm", ["install", "--recursive"], {
        cwd,
      });
      return;
    }
    if (this.npmClient == "npm") {
      execa.sync("npm", ["install"], {
        cwd,
      });
      this.commitAll();
      return;
    }

    for (const pkg of pkgs) {
      fs.symlinkSync(
        path.join(cwd, "packages", pkg),
        path.join(this.nodeModulesPath, pkg),
        "junction"
      );

      if (this.npmClient == "yarn") {
        const pkgJson = JSON.parse(
          fs.readFileSync(
            path.join(cwd, "packages", pkg, "package.json"),
            "utf-8"
          )
        );
        const deps = pkgJson.dependencies;

        yarnYaml += `\n"${pkg}@^${pkgJson.version}":\n  version "${pkgJson.version}"\n`;

        if (deps && Object.keys(deps).length > 0) {
          yarnYaml += `  dependencies:\n`;
          for (const dep of Object.keys(deps)) {
            yarnYaml += `    "${dep}" "0.1.0"\n`;
          }
        }
        this.commitFiles({ "yarn.lock": yarnYaml });
      }
    }
  }

  generateRepoFiles(turboConfig = {}) {
    this.commitFiles({
      [`.gitignore`]: `node_modules\n.turbo\n!*-lock.json`,
      "package.json": {
        name: this.name,
        version: "0.1.0",
        private: true,
        license: "MIT",
        workspaces: ["packages/**"],
        scripts: {
          build: `${turboPath} run build`,
          test: `${turboPath} run test`,
          lint: `${turboPath} run lint`,
        },
        turbo: {
          baseBranch: "origin/main",
          ...turboConfig,
        },
      },
    });
  }

  addPackage(name, internalDeps = []) {
    return this.commitFiles({
      [`packages/${name}/build.js`]: `
const fs = require('fs');
const path = require('path');
console.log('building ${name}');

if (!fs.existsSync(path.join(__dirname, 'dist'))){
   fs.mkdirSync(path.join(__dirname, 'dist'));
}

fs.copyFileSync(
  path.join(__dirname, 'build.js'),
  path.join(__dirname, 'dist', 'build.js')
);
`,
      [`packages/${name}/test.js`]: `console.log('testing ${name}');`,
      [`packages/${name}/lint.js`]: `console.log('linting ${name}');`,
      [`packages/${name}/package.json`]: {
        name,
        version: "0.1.0",
        license: "MIT",
        scripts: {
          build: "node ./build.js",
          test: "node ./test.js",
          lint: "node ./lint.js",
        },
        dependencies: {
          ...(internalDeps &&
            internalDeps.reduce((deps, dep) => {
              return {
                ...deps,
                [dep]: this.npmClient === "pnpm" ? "workspace:*" : "*",
              };
            }, {})),
        },
      },
    });
  }

  clone(origin) {
    return execa.sync("git", ["clone", origin], { cwd: this.root });
  }

  push(origin, branch) {
    return execa.sync("git", ["push", origin, branch], { cwd: this.root });
  }

  newBranch(branch) {
    return execa.sync("git", ["checkout", "-B", branch], { cwd: this.root });
  }

  commitFiles(files, options: { executable: boolean } = { executable: false }) {
    for (const [file, contents] of Object.entries(files)) {
      let out = "";
      if (typeof contents !== "string") {
        out = JSON.stringify(contents, null, 2);
      } else {
        out = contents;
      }

      const fullPath =
        this.subdir != null
          ? path.join(this.root, this.subdir, file)
          : path.join(this.root, file);

      if (!fs.existsSync(path.dirname(fullPath))) {
        fs.mkdirSync(path.dirname(fullPath), { recursive: true });
      }

      fs.writeFileSync(fullPath, out);

      if (options.executable) {
        fs.chmodSync(
          this.subdir != null
            ? path.join(this.root, this.subdir, file)
            : path.join(this.root, file),
          fs.constants.S_IXUSR | fs.constants.S_IRUSR | fs.constants.S_IROTH
        );
      }
    }
    execa.sync(
      "git",
      [
        "add",
        ...Object.keys(files).map((f) =>
          this.subdir != null
            ? path.join(this.root, this.subdir, f)
            : path.join(this.root, f)
        ),
      ],
      {
        cwd: this.root,
      }
    );
    return execa.sync("git", ["commit", "-m", "foo"], {
      cwd: this.root,
    });
  }

  commitAll() {
    execa.sync("git", ["add", "."], {
      cwd: this.root,
    });
    return execa.sync("git", ["commit", "-m", "foo"], {
      cwd: this.root,
    });
  }

  turbo(
    command,
    args?: readonly string[],
    options?: execa.SyncOptions<string>
  ) {
    return execa.sync(turboPath, [command, ...(args || [])], {
      cwd: this.root,
      shell: true,
      ...options,
    });
  }

  run(command, args?: readonly string[], options?: execa.SyncOptions<string>) {
    switch (this.npmClient) {
      case "yarn":
        return execa.sync("yarn", [command, ...(args || [])], {
          cwd: this.root,
          shell: true,
          ...options,
        });
      case "pnpm":
        return execa.sync("pnpm", [command, ...(args || [])], {
          cwd: this.root,
          shell: true,
          ...options,
        });
      case "npm":
        return execa.sync("npm", ["run", command, ...(args || [])], {
          cwd: this.root,
          shell: true,
          ...options,
        });
      default:
        throw new Error("npm client not implemented yet");
    }
  }

  readFileSync(filepath) {
    return fs.readFileSync(path.join(this.root, filepath), "utf-8");
  }

  cleanup() {
    fs.rmdirSync(this.root, { recursive: true });
  }
}
