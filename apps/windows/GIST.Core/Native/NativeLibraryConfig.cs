using System.Runtime.InteropServices;

// gist_ffi.dll may only be resolved from the application directory: never PATH, the current
// working directory or System32 (DLL-planting hardening, docs/windows-development-plan.md W1 item 6).
// The generated uniffi P/Invokes declare no DefaultDllImportSearchPaths of their own, so this
// assembly-level default governs them. AssemblyDirectory implies no PATH/CWD lookup.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]
