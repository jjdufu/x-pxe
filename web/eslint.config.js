import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

// 受限语法（step-11 第 10 节第 2 项）。每条规则带“豁免文件”：只有该文件可以使用对应 API。
const RESTRICTED = [
  { selector: "JSXOpeningElement[name.name='select']", message: "禁止原生 <select>，使用 Ark UI Select" },
  { selector: "JSXAttribute[name.name='as-child']", message: "React 只用 asChild，禁止 as-child（规范 §4.1）" },
  {
    selector: "CallExpression[callee.property.name='addEventListener'][callee.object.name=/^(document|window)$/]",
    message: "document/window 监听只能在 src/app/global-events.ts 注册",
    exempt: "src/app/global-events.ts",
  },
  { selector: "Identifier[name='localStorage']", message: "localStorage 只能经 src/lib/storage.ts 访问", exempt: "src/lib/storage.ts" },
  {
    selector: "MemberExpression[object.name='navigator'][property.name='clipboard']",
    message: "navigator.clipboard 只能经 src/lib/clipboard.ts 访问",
    exempt: "src/lib/clipboard.ts",
  },
  {
    // useEffect 中启动订阅或定时器却没有返回清理函数（规范 §3 STATE & LEAKS）
    selector:
      "CallExpression[callee.name='useEffect'] > ArrowFunctionExpression[body.type='BlockStatement']:not(:has(ReturnStatement)):has(CallExpression[callee.name=/^(setInterval|setTimeout|subscribeEvents|registerGlobalEvents|onDismissOverlays)$/], CallExpression[callee.property.name=/^(setInterval|setTimeout|addEventListener|subscribe)$/], NewExpression[callee.name='EventSource'])",
    message: "useEffect 中的订阅/定时器必须返回清理函数",
  },
];

const restrictedFor = (file) => ["error", ...RESTRICTED.filter((rule) => rule.exempt !== file).map(({ selector, message }) => ({ selector, message }))];

export default tseslint.config(
  { ignores: ["dist", "dist-gallery", "node_modules", "coverage", "test-results", "playwright-report"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.strictTypeChecked],
    languageOptions: {
      globals: globals.browser,
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/exhaustive-deps": "error",
      "no-alert": "error",
      "no-restricted-globals": ["error", { name: "confirm", message: "使用 AlertDialog" }, { name: "prompt", message: "使用 Dialog" }],
      "no-restricted-syntax": restrictedFor(null),
    },
  },
  ...RESTRICTED.filter((rule) => rule.exempt !== undefined).map((rule) => ({
    files: [rule.exempt],
    rules: { "no-restricted-syntax": restrictedFor(rule.exempt) },
  })),
  {
    // 测试代码需要直接操纵这些浏览器 API 来验证封装层；其余规则照常生效
    files: ["e2e/**/*.ts", "src/**/*.test.{ts,tsx}", "src/test-setup.ts"],
    rules: { "no-restricted-syntax": ["error", ...RESTRICTED.filter((rule) => rule.exempt === undefined).map(({ selector, message }) => ({ selector, message }))] },
  },
  {
    // Playwright 夹具的 use() 不是 React Hook
    files: ["e2e/**/*.ts"],
    rules: { "react-hooks/rules-of-hooks": "off" },
  },
  {
    files: ["**/*.{js,mjs}"],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["public/**/*.js"],
    languageOptions: { globals: globals.browser, sourceType: "script" },
  },
);
