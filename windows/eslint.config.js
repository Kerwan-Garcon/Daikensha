// ESLint for the island, settings and pill webviews (src/). Type-aware:
// typescript-eslint's strict and stylistic sets, with the options below
// chosen for this codebase. CI runs `npm run lint` with --max-warnings 0.
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "src-tauri/**", "node_modules/**", "target/**", "spikes/**", "dev/**", "scripts/**", "tests/**"] },
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  {
    files: ["src/**/*.ts"],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: {
      // `onclick: () => void Bridge.openUrl(url)` is the house idiom for
      // "fire and forget": the void operator says so explicitly.
      "@typescript-eslint/no-confusing-void-expression": ["error", { ignoreArrowShorthand: true, ignoreVoidOperator: true }],
      // Numbers in template strings (sizes, counts, CSS values) are intended.
      "@typescript-eslint/restrict-template-expressions": ["error", { allowNumber: true }],
      // `sync() {}` for a view with nothing to sync and `.catch(() => {})` for a
      // best-effort call (Bridge already logs the failure) are deliberate no-ops.
      "@typescript-eslint/no-empty-function": ["error", { allow: ["arrowFunctions", "methods"] }],
      // DOM listeners may be async; a rejection there reaches the console, and
      // every Bridge call already catches and logs its own errors.
      "@typescript-eslint/no-misused-promises": ["error", { checksVoidReturn: { arguments: false, attributes: false, properties: false } }],
      // Its fix turns `x as HTMLElement` into `x!`, which narrows to Element,
      // not HTMLElement, and fights no-non-null-assertion.
      "@typescript-eslint/non-nullable-type-assertion-style": "off",
      // `name || "file"` deliberately treats "" (and 0, false) as missing.
      // `const { [id]: _, ...rest } = obj` is how a key is dropped without `delete`.
      "@typescript-eslint/no-unused-vars": ["error", { ignoreRestSiblings: true }],
      "@typescript-eslint/prefer-nullish-coalescing": ["error", { ignorePrimitives: { string: true, number: true, boolean: true } }],
    },
  },
);
