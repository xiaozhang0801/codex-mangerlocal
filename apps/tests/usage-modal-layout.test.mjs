import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const usageModalSource = readFileSync(
  "src/components/modals/usage-modal.tsx",
  "utf8"
);
const globalsSource = readFileSync("src/app/globals.css", "utf8");

assert.match(
  usageModalSource,
  /<DialogContent[\s\S]*showCloseButton=\{false\}/,
  "Usage detail dialog should disable the default close button and own its close control."
);

assert.doesNotMatch(
  usageModalSource,
  /<DialogContent[\s\S]*style=\{\{/,
  "Usage detail dialog should use class-based viewport sizing instead of inline width/height."
);

assert.doesNotMatch(
  usageModalSource,
  /!h-\[min\(680px,calc\(100vh-2rem\)\)\]/,
  "Usage detail dialog should not use fixed center-height sizing that can be clipped by the desktop viewport."
);

assert.doesNotMatch(
  usageModalSource,
  /!bottom-4/,
  "Usage detail dialog should stay centered instead of pinning to the viewport bottom."
);

assert.doesNotMatch(
  usageModalSource,
  /!translate-y-0/,
  "Usage detail dialog should keep vertical center translation."
);

assert.match(
  usageModalSource,
  /usage-detail-dialog/,
  "Usage detail dialog should use the dedicated centered layout class."
);

assert.match(
  usageModalSource,
  /usage-detail-dialog__body/,
  "Usage detail dialog body should be the only scroll container."
);

assert.match(
  globalsSource,
  /\.usage-detail-dialog\s*\{[\s\S]*position:\s*fixed\s*!important;[\s\S]*top:\s*50%\s*!important;[\s\S]*bottom:\s*auto\s*!important;[\s\S]*height:\s*min\(620px,\s*calc\(100vh - 6rem\)\)\s*!important;/,
  "Usage detail dialog should have an explicit centered height that leaves viewport breathing room."
);

assert.match(
  globalsSource,
  /@supports \(height:\s*100dvh\)\s*\{[\s\S]*\.usage-detail-dialog\s*\{[\s\S]*height:\s*min\(620px,\s*calc\(100dvh - 6rem\)\)\s*!important;/,
  "Usage detail dialog should prefer dynamic viewport units when available."
);

assert.match(
  globalsSource,
  /\.usage-detail-dialog__body\s*\{[\s\S]*min-height:\s*0;[\s\S]*overflow-y:\s*auto;/,
  "Usage detail dialog body should shrink and scroll inside the fixed-height dialog."
);

assert.doesNotMatch(
  usageModalSource,
  /<DialogFooter[\s\S]*-mx-6/,
  "Usage detail footer should not use negative horizontal margins inside the fixed dialog grid."
);

assert.doesNotMatch(
  usageModalSource,
  /<DialogFooter[\s\S]*-mb-6/,
  "Usage detail footer should not use negative bottom margins inside the fixed dialog grid."
);

assert.match(
  usageModalSource,
  /absolute\s+right-4\s+top-4\s+z-20/,
  "Usage detail dialog should render a stable custom close button in the top-right corner."
);
