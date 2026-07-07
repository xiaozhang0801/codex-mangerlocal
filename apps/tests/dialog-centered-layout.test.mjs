import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const addAccountSource = readFileSync(
  "src/components/modals/add-account-modal.tsx",
  "utf8",
);
const aggregateApiSource = readFileSync(
  "src/components/modals/aggregate-api-modal.tsx",
  "utf8",
);
const onboardingSource = readFileSync(
  "src/components/layout/codex-cli-onboarding-dialog.tsx",
  "utf8",
);
const globalsSource = readFileSync("src/app/globals.css", "utf8");

assert.match(
  addAccountSource,
  /<DialogContent[\s\S]*app-centered-dialog/,
  "Add account dialog should opt into centered desktop viewport constraints.",
);

assert.match(
  addAccountSource,
  /app-centered-dialog__body/,
  "Add account dialog should make only the body scroll.",
);

assert.match(
  aggregateApiSource,
  /<DialogContent[\s\S]*app-centered-dialog/,
  "Aggregate API dialog should opt into centered desktop viewport constraints.",
);

assert.match(
  aggregateApiSource,
  /app-centered-dialog__body/,
  "Aggregate API dialog should make only the body scroll.",
);

assert.match(
  onboardingSource,
  /<DialogContent[\s\S]*app-centered-dialog/,
  "Codex onboarding dialog should opt into centered desktop viewport constraints.",
);

assert.match(
  onboardingSource,
  /app-centered-dialog__body/,
  "Codex onboarding dialog should make only the body scroll.",
);

assert.match(
  globalsSource,
  /\.app-centered-dialog\s*\{[\s\S]*position:\s*fixed\s*!important;[\s\S]*top:\s*50%\s*!important;[\s\S]*bottom:\s*auto\s*!important;[\s\S]*max-height:\s*calc\(100vh - 6rem\)\s*!important;/,
  "Centered app dialogs should force top-center positioning and leave bottom breathing room.",
);

assert.match(
  globalsSource,
  /@supports \(height:\s*100dvh\)\s*\{[\s\S]*\.app-centered-dialog\s*\{[\s\S]*max-height:\s*calc\(100dvh - 6rem\)\s*!important;/,
  "Centered app dialogs should prefer dynamic viewport height on desktop WebView.",
);

assert.match(
  globalsSource,
  /\.app-centered-dialog__body\s*\{[\s\S]*min-height:\s*0;[\s\S]*overflow-y:\s*auto;/,
  "Centered app dialog body should shrink and scroll inside the dialog.",
);
