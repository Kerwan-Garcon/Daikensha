#if ANDROID_LINK
import Foundation
import Security

// MARK: - Android link: its secrets in the Keychain, and nowhere else
//
// The pairing token and (later) the relay keys are generic passwords; the TLS key and its
// certificate are a key item and a certificate item, which the Keychain pairs into the
// `SecIdentity` Network.framework serves. All of them live in the data protection Keychain, tied to
// the signed app (no "Coucou wants to use…" prompt after each build), only on this Mac
// (`ThisDeviceOnly`, never synced). An unsigned build cannot use it: the link then does not start,
// and nothing is ever written to disk instead.

enum LinkKeychain {
    static let service = "fr.louisraille.NotchBuddy.android-link"
    static let identityLabel = "Coucou phone link"

    /// A readable sentence for the settings, never the item's content.
    static func describe(_ error: Error) -> String {
        if case LinkError.keychain(let status) = error {
            if status == errSecMissingEntitlement {
                return String(localized: "android.error.unsigned")
            }
            return String(format: String(localized: "android.error.keychain %lld"), Int64(status))
        }
        if case LinkError.noRandom = error { return String(localized: "android.error.random") }
        if case LinkError.noIdentity = error { return String(localized: "android.error.identity") }
        if case LinkError.cannotListen = error { return String(localized: "android.error.listen") }
        return String(localized: "android.error.identity")
    }
}

/// The token and the relay keys: generic passwords, one per name.
struct KeychainSecretStore: LinkSecretStore {
    func get(_ key: String) -> String? {
        var query = LinkKeychainQuery.password(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func set(_ key: String, _ value: String) throws {
        let query = LinkKeychainQuery.password(key)
        let update = SecItemUpdate(query as CFDictionary, [kSecValueData as String: Data(value.utf8)] as CFDictionary)
        if update == errSecSuccess { return }
        guard update == errSecItemNotFound else { throw LinkError.keychain(update) }
        var item = query
        item[kSecValueData as String] = Data(value.utf8)
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let added = SecItemAdd(item as CFDictionary, nil)
        guard added == errSecSuccess else { throw LinkError.keychain(added) }
    }

    func delete(_ key: String) {
        SecItemDelete(LinkKeychainQuery.password(key) as CFDictionary)
    }
}

enum LinkKeychainQuery {
    static func password(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: LinkKeychain.service,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: false,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }
}

/// The TLS key and certificate, as Keychain items the system pairs into a `SecIdentity`.
struct KeychainIdentityStore: LinkIdentityStore {
    private static let tag = Data("fr.louisraille.NotchBuddy.android-link.tls".utf8)

    private static var keyQuery: [String: Any] {
        [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    private static var certificateQuery: [String: Any] {
        [
            kSecClass as String: kSecClassCertificate,
            kSecAttrLabel as String: LinkKeychain.identityLabel,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    func load() -> LinkIdentity? {
        guard let identity = secIdentity() else { return nil }
        var certificate: SecCertificate?
        var key: SecKey?
        guard SecIdentityCopyCertificate(identity, &certificate) == errSecSuccess, let certificate,
              SecIdentityCopyPrivateKey(identity, &key) == errSecSuccess, let key,
              let raw = SecKeyCopyExternalRepresentation(key, nil) as Data? else { return nil }
        return LinkIdentity(certificate: Array(SecCertificateCopyData(certificate) as Data), privateKey: raw)
    }

    func save(_ identity: LinkIdentity) throws {
        // The old pair goes first: two identities with one label would be ambiguous.
        SecItemDelete(Self.keyQuery as CFDictionary)
        SecItemDelete(Self.certificateQuery as CFDictionary)
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
            kSecAttrKeySizeInBits as String: 256,
        ]
        guard let key = SecKeyCreateWithData(identity.privateKey as CFData, attributes as CFDictionary, nil),
              let certificate = SecCertificateCreateWithData(nil, Data(identity.certificate) as CFData) else {
            throw LinkError.noIdentity
        }
        // Key first: a certificate without its key is useless, the reverse is harmless.
        var keyItem = Self.keyQuery
        keyItem[kSecValueRef as String] = key
        keyItem[kSecAttrLabel as String] = LinkKeychain.identityLabel
        keyItem[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let addedKey = SecItemAdd(keyItem as CFDictionary, nil)
        guard addedKey == errSecSuccess else { throw LinkError.keychain(addedKey) }
        var certificateItem = Self.certificateQuery
        certificateItem[kSecValueRef as String] = certificate
        certificateItem[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let addedCertificate = SecItemAdd(certificateItem as CFDictionary, nil)
        guard addedCertificate == errSecSuccess else { throw LinkError.keychain(addedCertificate) }
    }

    /// The identity Network.framework serves: the certificate labelled for the link, with its key.
    func secIdentity() -> SecIdentity? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassIdentity,
            kSecAttrLabel as String: LinkKeychain.identityLabel,
            kSecReturnRef as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecUseDataProtectionKeychain as String: true,
        ]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let result,
              CFGetTypeID(result) == SecIdentityGetTypeID() else { return nil }
        return (result as! SecIdentity)
    }

    /// Whether the data protection Keychain can be used at all (an unsigned build cannot).
    static func check() throws {
        var query = LinkKeychainQuery.password("phone-link-probe")
        query[kSecReturnData as String] = true
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecMissingEntitlement { throw LinkError.keychain(status) }
    }
}
#endif

#if ANDROID_LINK
/// One line in ~/Library/Logs/NotchBuddy/nb.log. Only states: never a token, a key, an address or a command.
func linkLog(_ message: String) {
    appendAppLog("nb.log", "[Android] \(message)")
}
#endif
