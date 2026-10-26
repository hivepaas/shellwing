import Foundation
import Security

/// The vault's key in the iOS Keychain: one generic password, readable after
/// the phone was first unlocked since it started, and on this phone only
/// (never in a backup restored elsewhere). The vault asks from its own thread.
final class KeychainKeys: PlatformKeys, @unchecked Sendable {
    private let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "com.hivepaas.shellwing.vault",
        kSecAttrAccount as String: "account key",
    ]

    func load() throws -> Data? {
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        switch status {
        case errSecSuccess: return out as? Data
        case errSecItemNotFound: return nil
        default: throw MobileError.Failed(Self.said(status))
        }
    }

    func save(key: Data) throws {
        let status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: key] as CFDictionary)
        if status == errSecItemNotFound {
            var add = query
            add[kSecValueData as String] = key
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let added = SecItemAdd(add as CFDictionary, nil)
            guard added == errSecSuccess else { throw MobileError.Failed(Self.said(added)) }
        } else if status != errSecSuccess {
            throw MobileError.Failed(Self.said(status))
        }
    }

    func forget() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw MobileError.Failed(Self.said(status))
        }
    }

    private static func said(_ status: OSStatus) -> String {
        (SecCopyErrorMessageString(status, nil) as String?) ?? "the Keychain said \(status)"
    }
}
