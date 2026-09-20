# PENDING_WINDOWS_CHANGES.md

Windows-platform work identified during a non-Windows session that must be applied or
verified in the next Windows session. Typical cause: a macOS session edited Windows-only
C#/XAML under `apps/windows/**` as text (allowed, with a warning) but could not build or
test it. On Windows and Linux, the SessionStart hook (`tools/detect-platform.sh`) flags
this file when it contains entries.

**Delete each entry's block after the change is applied/verified and committed.**

<!-- Template — copy to add an entry (real headings start at column 0 with "## Pending Windows Change"; this example is indented so the detector ignores it):

    ## Pending Windows Change — [YYYY-MM-DD]
**File:** [path]
**Change required:** [what]
**Reason:** [why]
**Related commit/PR:** [hash or PR]
**Action:** [steps to apply / build / test on Windows]

-->

<!-- No pending items. -->
