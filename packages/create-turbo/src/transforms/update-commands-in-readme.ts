import path from "node:path";
import fs from "node:fs/promises";
import { TransformError } from "./errors";
import type { TransformInput, TransformResult} from "./types";

const meta = {
  name: "update-commands-in-readme",
};

export async function transform(args: TransformInput): TransformResult {
  const { prompts, example } = args;

  const selectedPackageManager = prompts.packageManager;
  const readmeFilePath = path.join(prompts.root, "README.md");
  try {
    // Read the content of the file
    let data = await fs.readFile(readmeFilePath, "utf8");

    // replace package manager
    const updatedReadmeData = replacePackageManager(selectedPackageManager, data);    

    // Write the updated content back to the file
    await fs.writeFile(readmeFilePath, updatedReadmeData, "utf8");

  } catch (err) {
    throw new TransformError("Unable to update README.md", {
      transform: meta.name,
      fatal: false,
    });
  }
  return { result: "success", ...meta };
}

function replacePackageManager(packageManager, text) {
  // an array of all the possible replacement strings.
  const replacements = ['pnpm run', 'npm run', 'yarn run', 'bun run', 'pnpm', 'npm', 'yarn', 'bun'];
  
  // regex to search for a pattern enclosed in single backticks (` `), double backticks (`` ``) or
  // triple backticks (``` ```) considering there might be newlines in between backticks and commands.
  const searchRegex = new RegExp(`\`\`\`[\\s\\S]*?\\b(?:${replacements.join('|')})\\b[\\s\\S]*?\`\`\`|\`\`[\\s\\S]*?\\b(?:${replacements.join('|')})\\b[\\s\\S]*?\`\`|\`[\\s\\S]*?\\b(?:${replacements.join('|')})\\b[\\s\\S]*?\``, 'g');

  // Replace all occurrences of regex with selectedPackageManager
  text = text.replace(searchRegex, (match) => {
      // replacement regex => the regex required to replace the package manager.
      const replacementRegex = new RegExp(`\\b(?:${replacements.join('|')})\\b`, 'g');
      const updatedText = match.replace(replacementRegex, `${packageManager} run`);
      return updatedText;
  });
  return text;
}