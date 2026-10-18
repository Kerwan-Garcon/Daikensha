import Foundation
import Synchronization

// Stand-ins for the Keychain, in memory, for the tests and the interop server.

final class MemorySecretStore: LinkSecretStore {
    let values = Mutex<[String: String]>([:])
    /// Every write fails, like a locked Keychain.
    let refuseWrites = Atomic<Bool>(false)

    func get(_ key: String) -> String? { values.withLock { $0[key] } }

    func set(_ key: String, _ value: String) throws {
        if refuseWrites.load(ordering: .sequentiallyConsistent) { throw LinkError.keychain(-25308) }
        values.withLock { $0[key] = value }
    }
}

final class MemoryIdentityStore: LinkIdentityStore {
    let stored = Mutex<LinkIdentity?>(nil)

    func load() -> LinkIdentity? { stored.withLock { $0 } }
    func save(_ identity: LinkIdentity) throws { stored.withLock { $0 = identity } }
}
