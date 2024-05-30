import path from "node:path";
import { readJsonSync, existsSync } from "fs-extra";
import type { RootSchema } from "@turbo/types/src/types/config";
import type { Transformer, TransformerArgs } from "../types";
import { getTransformerHelpers } from "../utils/getTransformerHelpers";
import type { TransformerResults } from "../runner";

// transformer details
const TRANSFORMER = "stabilize-ui";
const DESCRIPTION = 'Rename the "experimentalUI" key to "ui" in `turbo.json`';
const INTRODUCED_IN = "2.0.0";

interface ExperimentalSchema extends RootSchema {
  experimentalUI?: boolean;
}

function migrateConfig(config: ExperimentalSchema): RootSchema {
  const ui = config.experimentalUI;
  delete config.experimentalUI;
  // If UI is enabled we can just remove the config now that it's enabled by default
  if (ui !== undefined && !ui) {
    config.ui = ui;
  }
  return config;
}

export function transformer({
  root,
  options,
}: TransformerArgs): TransformerResults {
  const { log, runner } = getTransformerHelpers({
    transformer: TRANSFORMER,
    rootPath: root,
    options,
  });

  log.info(`Renaming \`experimentalUI\` key in turbo.json to \`ui\``);
  const turboConfigPath = path.join(root, "turbo.json");
  if (!existsSync(turboConfigPath)) {
    return runner.abortTransform({
      reason: `No turbo.json found at ${root}. Is the path correct?`,
    });
  }

  const turboJson = readJsonSync(turboConfigPath) as RootSchema;
  runner.modifyFile({
    filePath: turboConfigPath,
    after: migrateConfig(turboJson),
  });

  return runner.finish();
}

const transformerMeta: Transformer = {
  name: TRANSFORMER,
  description: DESCRIPTION,
  introducedIn: INTRODUCED_IN,
  transformer,
};

// eslint-disable-next-line import/no-default-export -- transforms require default export
export default transformerMeta;
