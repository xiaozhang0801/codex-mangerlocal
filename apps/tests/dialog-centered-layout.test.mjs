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
const apiKeySource = readFileSync(
  "src/components/modals/api-key-modal.tsx",
  "utf8",
);
const onboardingSource = readFileSync(
  "src/components/layout/codex-cli-onboarding-dialog.tsx",
  "utf8",
);
const accountEditorSource = readFileSync(
  "src/app/accounts/accounts-page-view.tsx",
  "utf8",
);
const aggregateApiPageSource = readFileSync(
  "src/app/aggregate-api/page.tsx",
  "utf8",
);
const modelCatalogSource = readFileSync(
  "src/components/modals/model-catalog-modal.tsx",
  "utf8",
);
const modelsPageSource = readFileSync("src/app/models/page.tsx", "utf8");
const settingsTasksSource = readFileSync(
  "src/app/settings/components/tasks-tab-content.tsx",
  "utf8",
);
const accountManagerSource = readFileSync(
  "src/app/account-manager/page.tsx",
  "utf8",
);
const modelGroupsSource = readFileSync(
  "src/app/model-groups/page.tsx",
  "utf8",
);
const globalsSource = readFileSync("src/app/globals.css", "utf8");

function assertCenteredDialogNear(source, marker, label) {
  const markerIndex = source.indexOf(marker);
  assert.notEqual(markerIndex, -1, `${label} marker should exist.`);
  const dialogStart = source.lastIndexOf("<DialogContent", markerIndex);
  assert.notEqual(dialogStart, -1, `${label} should render DialogContent.`);
  const snippet = source.slice(dialogStart, markerIndex + 5000);
  assert.match(
    snippet,
    /<DialogContent[\s\S]*app-centered-dialog/,
    `${label} should opt into centered desktop viewport constraints.`,
  );
  assert.match(
    snippet,
    /app-centered-dialog__body/,
    `${label} should make only the body scroll.`,
  );
}

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
  apiKeySource,
  /<DialogContent[\s\S]*app-centered-dialog/,
  "Platform key dialog should opt into centered desktop viewport constraints.",
);

assert.match(
  apiKeySource,
  /app-centered-dialog__body/,
  "Platform key dialog should make only the body scroll.",
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

assertCenteredDialogNear(
  accountEditorSource,
  '<DialogTitle>{t("编辑账号信息")}</DialogTitle>',
  "Account editor dialog",
);

assertCenteredDialogNear(
  aggregateApiPageSource,
  '<DialogTitle>{t("模型池配置")}</DialogTitle>',
  "Aggregate API model pool dialog",
);

assertCenteredDialogNear(
  modelCatalogSource,
  "核心字段单独编辑",
  "Model catalog edit dialog",
);

assertCenteredDialogNear(
  modelsPageSource,
  '<DialogTitle>{t("关联来源")}</DialogTitle>',
  "Model source routing dialog",
);

assertCenteredDialogNear(
  settingsTasksSource,
  '<DialogTitle>{t("高级参数")}</DialogTitle>',
  "Worker advanced settings dialog",
);

assertCenteredDialogNear(
  accountManagerSource,
  '<DialogTitle>{t("成员用量详情")}</DialogTitle>',
  "Member usage detail dialog",
);

assertCenteredDialogNear(
  modelGroupsSource,
  '<DialogTitle>{editingGroup ? t("管理模型组") : t("新建模型组")}</DialogTitle>',
  "Model group management dialog",
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
