// probe_metal.swift — settle umem design questions on this Mac.
import Metal
import Foundation

let dev = MTLCreateSystemDefaultDevice()!
print("device: \(dev.name)")
print("maxBufferLength GiB: \(Double(dev.maxBufferLength) / 1073741824)")
print("recommendedMaxWorkingSetSize GiB: \(Double(dev.recommendedMaxWorkingSetSize) / 1073741824)")
print("hasUnifiedMemory: \(dev.hasUnifiedMemory)")

func tryNoCopy(_ gib: Int) {
    let n = gib << 30
    guard let p = mmap(nil, n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0), p != MAP_FAILED else { print("mmap fail"); return }
    // touch a few pages so it's real
    let bytes = p.bindMemory(to: UInt8.self, capacity: n)
    for i in stride(from: 0, to: n, by: 1 << 24) { bytes[i] = 1 }
    let b = dev.makeBuffer(bytesNoCopy: p, length: n, options: .storageModeShared, deallocator: nil)
    print("\(gib) GiB NoCopy buffer: \(b != nil ? "OK" : "FAILED")")
    if let b = b {
        // Verify GPU sees CPU writes: trivial kernel-free check via blit sum isn't available; use contents pointer identity
        print("  contents == mmap ptr: \(b.contents() == p)")
    }
    munmap(p, n)
}
tryNoCopy(1)
tryNoCopy(5)
tryNoCopy(12)

// File-backed mapping → Metal buffer
let path = "/tmp/umem_probe.bin"
let fsz = 1 << 30
FileManager.default.createFile(atPath: path, contents: Data(count: fsz))
let fd = open(path, O_RDONLY)
if let fp = mmap(nil, fsz, PROT_READ, MAP_PRIVATE, fd, 0), fp != MAP_FAILED {
    let b = dev.makeBuffer(bytesNoCopy: fp, length: fsz, options: .storageModeShared, deallocator: nil)
    print("file-backed 1 GiB (PROT_READ, MAP_PRIVATE) NoCopy: \(b != nil ? "OK" : "FAILED")")
    munmap(fp, fsz)
}
close(fd); unlink(path)
