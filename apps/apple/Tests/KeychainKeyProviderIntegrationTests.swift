import XCTest
import Security
@testable import GIST

/// **Verification spike for A6** (see CLAUDE.md's security register):
/// exercises `KeychainKeyProvider` against the REAL macOS Keychain — no
/// mocking, matching this codebase's existing preference for real
/// integration-style tests (see `GISTTests`'s own doc comment). This is the
/// one thing ADR-011 explicitly flagged as not yet runtime-verified before
/// any production wiring work proceeds.
///
/// Every Keychain item this test creates is scoped to a UUID-suffixed
/// `service` string that can never collide with the real production
/// identifiers (`"com.gist.macos.encryption-at-rest"` /
/// `"document-content-key"`, both still the defaults on
/// `KeychainKeyProvider.init` — untouched by this file). `CoreClient.shared`
/// is never constructed or touched here.
///
/// Whether this file stays in the suite long-term depends on whether it ran
/// cleanly and non-interactively — see the note this test's run added to
/// CLAUDE.md's `A6` register row for the actual verdict.
final class KeychainKeyProviderIntegrationTests: XCTestCase {
    private var testService: String!
    private var testAccount: String!
    private var provider: KeychainKeyProvider!

    override func setUpWithError() throws {
        try super.setUpWithError()
        testService = "com.gist.macos.encryption-at-rest.TEST-\(UUID().uuidString)"
        testAccount = "document-content-key-TEST"
        provider = KeychainKeyProvider(service: testService, account: testAccount)
    }

    override func tearDownWithError() throws {
        // Best-effort cleanup so nothing test-related is left behind in the
        // real Keychain after this run. `KeychainKeyProvider` exposes no
        // delete/reset method (by design — production code never needs
        // one), so this deletes directly via `Security`, using the exact
        // same service/account this test constructed the provider with.
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: testService as Any,
            kSecAttrAccount as String: testAccount as Any,
        ]
        SecItemDelete(query as CFDictionary)

        provider = nil
        testService = nil
        testAccount = nil
        try super.tearDownWithError()
    }

    /// Basic round trip: first call creates+persists a 32-byte key, second
    /// call reads the same bytes back rather than generating a new one.
    func testGetOrCreateKeyReturns32BytesAndPersistsAcrossCalls() {
        let first = provider.getOrCreateKey()
        XCTAssertEqual(first.count, 32, "AES-256 key must be exactly 32 bytes")

        let second = provider.getOrCreateKey()
        XCTAssertEqual(second.count, 32)
        XCTAssertEqual(
            first, second,
            "second call must read back the persisted key, not generate a fresh one"
        )
    }

    /// Directly exercises the `SecItemAdd`-race fix documented in
    /// `KeychainKeyProvider.store(key:)`: many concurrent first-time calls
    /// race to create the same Keychain item; exactly one `SecItemAdd`
    /// wins, and every losing call must re-read and return the winner's
    /// key, never its own locally-generated, never-persisted candidate.
    /// Before the fix, this would have produced more than one distinct key
    /// across the group.
    func testConcurrentGetOrCreateKeyCallsConvergeOnOneKey() {
        let provider = self.provider!
        let iterations = 20
        var results = [Data](repeating: Data(), count: iterations)
        let lock = NSLock()

        DispatchQueue.concurrentPerform(iterations: iterations) { index in
            let key = provider.getOrCreateKey()
            lock.lock()
            results[index] = key
            lock.unlock()
        }

        XCTAssertEqual(results.count, iterations)
        for key in results {
            XCTAssertEqual(key.count, 32, "every concurrent call must still return a 32-byte key")
        }
        XCTAssertEqual(
            Set(results).count, 1,
            "all concurrent calls must converge on exactly one persisted key — a losing " +
            "SecItemAdd caller returning its own candidate instead of re-reading the winner " +
            "would show up here as more than one distinct key"
        )
    }
}
