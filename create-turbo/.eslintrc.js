module.exports = {
  root: true,
  parser: "@typescript-eslint/parser",
  env: { node: true },
  extends: ["eslint:recommended"],
  rules: {
    "no-empty": ["error", { allowEmptyCatch: true }],
  },
};
