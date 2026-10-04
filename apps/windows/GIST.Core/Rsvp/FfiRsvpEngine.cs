using uniffi.gist_ffi;

namespace Gist.Core.Rsvp;

/// <summary>
/// <see cref="IRsvpEngine"/> over the Rust <c>FfiRsvpSession</c>. Maps the generated records to the
/// public ones and does nothing else: no pacing arithmetic lives here (plan §4.3).
/// </summary>
internal sealed class FfiRsvpEngine : IRsvpEngine
{
    private readonly FfiRsvpSession _session;
    private bool _disposed;

    public FfiRsvpEngine(FfiRsvpSession session)
    {
        ArgumentNullException.ThrowIfNull(session);
        _session = session;
    }

    public ulong TokenCount => _session.TokenCount();

    public ulong Cursor => _session.Cursor();

    public uint Wpm => _session.Wpm();

    public RsvpFrame? FrameAtElapsed(ulong elapsedMs)
    {
        var f = _session.FrameAtElapsed(elapsedMs);
        return f is null
            ? null
            : new RsvpFrame(f.Index, f.Text, Map(f.Kind), f.DurationMs, f.NextBoundaryMs, f.IsLast, f.TokenCount);
    }

    public string? TokenText(ulong index) => _session.TokenText(index);

    public bool IsWord(ulong index) => _session.TokenKind(index) is { } k && k == FfiTokenKind.Word;

    public void Seek(ulong index) => _session.Seek(index);

    public void Pause(ulong elapsedMs) => _session.Pause(elapsedMs);

    public void Resume() => _session.Resume();

    public void BackWords(ulong count, ulong elapsedMs) => _session.BackWords(count, elapsedMs);

    public void SetWpm(uint wpm, ulong elapsedMs) => _session.SetWpm(wpm, elapsedMs);

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _session.Dispose();
    }

    private static RsvpTokenKind Map(FfiTokenKind kind) => kind switch
    {
        FfiTokenKind.Word => RsvpTokenKind.Word,
        FfiTokenKind.ParagraphBreak => RsvpTokenKind.ParagraphBreak,
        _ => RsvpTokenKind.SectionBreak,
    };
}
