// List notarytool profile names from attributes; never request password data.
import Foundation
import Security
import LocalAuthentication

var profiles = Set<String>()
let prefix = "com.apple.gke.notary.tool.saved-creds."
for dataProtection in [true, false] {
    let context = LAContext()
    context.interactionNotAllowed = true
    var query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "com.apple.gke.notary.tool",
        kSecReturnAttributes as String: true,
        kSecMatchLimit as String: kSecMatchLimitAll,
        kSecUseAuthenticationContext as String: context,
    ]
    if dataProtection { query[kSecUseDataProtectionKeychain as String] = true }
    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    if status == errSecItemNotFound { continue }
    guard status == errSecSuccess else {
        fputs("Cannot inspect notarization profile attributes; set NOTARY_PROFILE explicitly.\n", stderr)
        exit(1)
    }
    for item in result as? [[String: Any]] ?? [] {
        if let account = item[kSecAttrAccount as String] as? String, account.hasPrefix(prefix) {
            profiles.insert(String(account.dropFirst(prefix.count)))
        }
    }
}
let data = try JSONSerialization.data(withJSONObject: profiles.sorted())
FileHandle.standardOutput.write(data)
