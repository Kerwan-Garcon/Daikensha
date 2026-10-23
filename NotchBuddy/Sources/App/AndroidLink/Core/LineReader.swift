#if ANDROID_LINK
import Foundation

// MARK: - Android link: newline-delimited lines, none over 64 KiB
//
// Bytes come in as the transport delivers them; whole lines go out. A line longer than
// `LinkProtocol.maxLine` (newline and an optional `\r` not counted) is a protocol error, and the
// reader never holds more than two bytes past the limit, however much is sent. Empty lines are
// skipped. Same rules as `LineReader` in server.rs.

struct LineReader {
    enum Next: Equatable {
        case line([UInt8])
        case tooLong
    }

    private var buffer: [UInt8] = []
    private var failed = false
    let limit: Int

    init(limit: Int = LinkProtocol.maxLine) {
        self.limit = limit
    }

    /// The lines completed by these bytes. After `.tooLong` nothing else is ever returned.
    mutating func feed(_ bytes: some Collection<UInt8>) -> [Next] {
        guard !failed else { return [] }
        var out: [Next] = []
        for byte in bytes {
            if byte == UInt8(ascii: "\n") {
                var line = buffer
                buffer.removeAll(keepingCapacity: true)
                if line.last == UInt8(ascii: "\r") { line.removeLast() }
                if line.count > limit {
                    failed = true
                    out.append(.tooLong)
                    return out
                }
                if !line.isEmpty { out.append(.line(line)) }
                continue
            }
            buffer.append(byte)
            // Content + "\r" + "\n" is at most limit + 2 bytes; this many with no newline yet cannot fit.
            if buffer.count >= limit + 2 {
                failed = true
                buffer.removeAll()
                out.append(.tooLong)
                return out
            }
        }
        return out
    }
}
#endif
