import postcss from "@vercel/turbopack/postcss";
import importedConfig from "CONFIG";
import { relative, isAbsolute, sep } from "path";

const contextDir = process.cwd();
const toPath = (file) => {
  const relPath = relative(contextDir, file);
  if (isAbsolute(relPath)) {
    throw new Error(
      `Cannot depend on path (${file}) outside of root directory (${contextDir})`
    );
  }
  return sep !== "/" ? relPath.replaceAll(sep, "/") : relPath;
};

const transform = async (ipc, cssContent, name) => {
  let config = importedConfig;
  if (typeof config === "function") {
    config = await config({ env: "development" });
  }
  let plugins = config.plugins;
  if (Array.isArray(plugins)) {
    plugins = plugins.map((plugin) => {
      if (Array.isArray(plugin)) {
        return plugin;
      } else if (typeof plugin === "string") {
        return [plugin, {}];
      } else {
        return plugin;
      }
    });
  } else if (typeof plugins === "object") {
    plugins = Object.entries(plugins).filter(([, options]) => options);
  }
  const loadedPlugins = plugins.map((plugin) => {
    if (Array.isArray(plugin)) {
      const [arg, options] = plugin;
      let pluginFactory = arg;

      if (typeof pluginFactory === "string") {
        pluginFactory = __turbopack_external_require__(pluginFactory);
      }

      if (pluginFactory.default) {
        pluginFactory = pluginFactory.default;
      }

      return pluginFactory(options);
    }
    return plugin;
  });

  const processor = postcss(loadedPlugins);
  const { css, map, messages } = await processor.process(cssContent, {
    from: name,
    to: name,
    map: {
      inline: false,
    },
  });

  const assets = [];
  for (const msg of messages) {
    console.error(msg);
    switch (msg.type) {
      case "asset":
        assets.push({
          file: msg.file,
          content: msg.content,
          sourceMap:
            typeof msg.sourceMap === "string"
              ? msg.sourceMap
              : JSON.stringify(msg.sourceMap),
          // There is also an info field, which we currently ignore
        });
        break;
      case "file-dependency":
      case "missing-dependency":
        ipc.send({
          type: "fileDependency",
          path: toPath(msg.file),
        });
        break;
      case "build-dependency":
        ipc.send({
          type: "buildDependency",
          path: toPath(msg.file),
        });
        break;
      case "dir-dependency":
        ipc.send({
          type: "dirDependency",
          path: toPath(msg.dir),
          glob: msg.glob,
        });
        break;
      case "context-dependency":
        ipc.send({
          type: "dirDependency",
          path: toPath(msg.file),
          glob: "**",
        });
        break;
    }
  }
  return {
    css,
    map: JSON.stringify(map),
    assets,
  };
};

export { transform as default };
