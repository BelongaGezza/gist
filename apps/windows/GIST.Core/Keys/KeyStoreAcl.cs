using System.Security.AccessControl;
using System.Security.Principal;

namespace Gist.Core.Keys;

/// <summary>
/// Defence-in-depth ACL hardening for the key directory (review finding Q9: the spike set no ACL at
/// all). DPAPI, not the ACL, is the confidentiality control — a same-user process can unwrap the blob
/// regardless — so every operation here is best-effort and never fails the caller.
/// </summary>
/// <remarks>
/// Only the <b>directory</b> is hardened, with inheritable rules; the key file and any scratch file
/// then pick the ACL up by inheritance (and Windows propagates it to a key file that already exists).
/// Setting the file's own DACL as well was tried and reverted: <c>SetAccessControl</c> opens the file
/// in a way that makes a concurrently-racing reader fail with a sharing violation, which is exactly
/// the create race ADR-016 is careful about.
/// </remarks>
internal static class KeyStoreAcl
{
    /// <summary>
    /// Removes inherited permissions from the directory and leaves exactly one inheritable rule:
    /// Full Control for the current user. A directory that is already protected is left alone, so
    /// this is a cheap no-op in the steady state and does not re-propagate over live files.
    /// </summary>
    /// <returns><c>true</c> if the directory is restricted when this returns; <c>false</c> if the platform refused.</returns>
    internal static bool TryRestrictDirectoryToCurrentUser(string path)
    {
        try
        {
            var info = new DirectoryInfo(path);
            if (!info.Exists) return false;
            if (info.GetAccessControl(AccessControlSections.Access).AreAccessRulesProtected) return true;

            var user = WindowsIdentity.GetCurrent().User;
            if (user is null) return false;

            var security = new DirectorySecurity();
            // (true, false): protect from inheritance, and do NOT copy the inherited rules down.
            security.SetAccessRuleProtection(true, false);
            security.AddAccessRule(new FileSystemAccessRule(
                user,
                FileSystemRights.FullControl,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow));
            info.SetAccessControl(security);
            return true;
        }
        catch (Exception e) when (e is UnauthorizedAccessException or PrivilegeNotHeldException
                                  or IOException or PlatformNotSupportedException or NotSupportedException
                                  or IdentityNotMappedException or ArgumentException or InvalidOperationException)
        {
            // A filesystem without ACL support, a locked-down profile, or a child held open by another
            // process while inheritance propagates. The key stays DPAPI-protected either way;
            // hardening is a bonus, not a precondition.
            return false;
        }
    }
}
