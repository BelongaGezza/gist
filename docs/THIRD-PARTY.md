# Third-party dependencies

## NuGet packages (Windows app, `apps/windows`)

Versions are pinned in `apps/windows/Directory.Packages.props`. Licences below were read from each
package's `.nuspec` on nuget.org (and, for the file-based one, from the `license.txt` inside the
`.nupkg`), not inferred.

| Package | Version | Licence | Notes |
|---|---|---|---|
| System.Security.Cryptography.ProtectedData | 10.0.12 | MIT | SPDX expression in nuspec |
| CommunityToolkit.Mvvm | 8.4.2 | MIT | SPDX expression in nuspec |
| Microsoft.WindowsAppSDK | 2.5.1 | Microsoft Software License Terms, "Microsoft Windows App SDK" (proprietary, `license.txt` in package) | Not an OSI licence. Permits install/use to develop and test applications solely for Windows; review the redistribution terms in the package's `license.txt` before shipping (M4/MSIX release, ADR-017). Not covered by the Rust `cargo-deny` allow-list. |
| Microsoft.NET.Test.Sdk | 17.14.1 | MIT | Test-only, not shipped |
| xunit | 2.9.3 | Apache-2.0 | Test-only, not shipped |
| xunit.runner.visualstudio | 2.8.2 | Apache-2.0 | Test-only, not shipped |
| FlaUI.Core | 5.0.0 | MIT | `LICENSE.txt` in the package read; test-only (GIST.App.UITests), not shipped |
| FlaUI.UIA3 | 5.0.0 | MIT | `LICENSE.txt` in the package read; brings Interop.UIAutomationClient (transitive, unreviewed); test-only, not shipped |

Transitive NuGet packages are not listed here; `dotnet list package --include-transitive` in
`windows-build` CI covers vulnerabilities, and licences of transitive packages should be reviewed
when the first release build is produced.
