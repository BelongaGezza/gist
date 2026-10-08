#!/usr/bin/env bash
# Fails if a user-facing string literal in the Apple sources is missing from
# apps/apple/Shared/Localizable.xcstrings.
#
# Why this exists: the build does not write extracted keys back into the
# catalog (SWIFT_EMIT_LOC_STRINGS only emits .stringsdata), so catalog entries
# are added by hand and can silently drift. See docs/BUILDING-macos.md,
# "Localisation".
#
# What it checks: every string literal passed directly to a known
# user-facing API (Text, Button, Label, Toggle, Picker, Section, Menu,
# TextField, .help, .accessibilityLabel/Value/Hint, .navigationTitle,
# .alert, .confirmationDialog, .searchable prompt, String(localized:),
# LocalizedStringResource, ...) exists as a catalog key. Interpolations
# (\(x)) are matched against catalog keys with format specifiers
# (%lld / %@ / %1$lld ...).
#
# What it does NOT check (limits, by design): strings built into a plain
# String variable and shown via a String-typed parameter (these bypass
# localisation entirely and cannot be found textually -- wrap them in
# String(localized:)); whether a catalog key is still used; whether the
# specifier type (%lld vs %@) matches the interpolated expression's type;
# translations (none exist).
#
# Usage: tools/check-localisation.sh   (from anywhere; no arguments)
# Needs: bash, perl. No python3 (see CLAUDE.md).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CATALOG="$ROOT/apps/apple/Shared/Localizable.xcstrings"
[ -f "$CATALOG" ] || { echo "check-localisation: catalog not found: $CATALOG" >&2; exit 2; }

FILES=()
while IFS= read -r f; do FILES+=("$f"); done < <(
  find "$ROOT/apps/apple/Shared" "$ROOT/apps/apple/macOS" -name '*.swift' | sort
)

exec perl -e '
use strict; use warnings; use utf8;
binmode(STDOUT, ":utf8"); binmode(STDERR, ":utf8");
my $cat = shift @ARGV;
open(my $c, "<:encoding(UTF-8)", $cat) or die "cannot read $cat";
my $json = do { local $/; <$c> }; close $c;

sub norm_key {
  my $k = shift;
  $k =~ s/%(?:\d+\$)?(?:lld|ld|d|@|f|lf|u)/\x{1}/g;
  $k =~ s/%%/%/g;
  return $k;
}
my %keys;
# Xcode-formatted catalog: top-level keys sit at exactly 4 spaces of indent.
while ($json =~ /^    "((?:[^"\\]|\\.)*)" : \{/mg) {
  my $k = $1;
  $k =~ s/\\"/"/g; $k =~ s/\\n/\n/g; $k =~ s/\\\\/\\/g;
  $keys{norm_key($k)} = 1;
}
die "no keys parsed from catalog (format changed?)" unless %keys;

# A balanced \( ... ) interpolation that may itself contain string literals.
my $interp = qr/\\(\((?:[^()"]|"(?:[^"\\]|\\.)*"|(?-1))*\))/;
my $lit    = qr/"((?:[^"\\\n]|$interp|\\.)*)"/;
my $api = qr/(?:\bText|\bButton|\bLabel|\bToggle|\bPicker|\bSection|\bMenu|\bTextField|\bSecureField|\bLabeledContent|\bStepper|\bProgressView|\bLink|\bDisclosureGroup|\bContentUnavailableView|\bGroupBox|\bNavigationLink|\.help|\.accessibilityLabel|\.accessibilityValue|\.accessibilityHint|\.navigationTitle|\.navigationSubtitle|\.alert|\.confirmationDialog|\.searchable|\.badge|\bTab|\blocalized:|\bLocalizedStringResource|\bCommandMenu|\bprompt:|\btitle:|\bmessage:)\s*\(?\s*(?:[a-zA-Z]+:\s*)?/;

my ($checked, $missing) = (0, 0);
for my $f (@ARGV) {
  open(my $h, "<:encoding(UTF-8)", $f) or die "cannot read $f";
  my $text = do { local $/; <$h> }; close $h;
  # Blank out full-line comments (keep newlines so line numbers stay right).
  $text =~ s{^([ \t]*//[^\n]*)$}{" " x length($1)}mge;
  while ($text =~ /($api)$lit/g) {
    my $body = $2;
    my $line = 1 + (substr($text, 0, $-[0]) =~ tr/\n//);
    my $s = $body;
    $s =~ s/$interp/\x{1}/g;
    $s =~ s/\\u\{([0-9A-Fa-f]+)\}/chr(hex($1))/ge;
    $s =~ s/\\"/"/g; $s =~ s/\\n/\n/g; $s =~ s/\\\\/\\/g;
    $s =~ s/%%/%/g;
    next if $s eq "";
    $checked++;
    next if $keys{$s};
    $missing++;
    (my $rel = $f) =~ s{.*/apps/apple/}{apps/apple/};
    print "MISSING from catalog: $rel:$line: \"$body\"\n";
  }
}
print "check-localisation: $checked literals checked, $missing missing\n";
exit($missing ? 1 : 0);
' "$CATALOG" "${FILES[@]}"
