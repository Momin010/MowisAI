import Foundation
import Virtualization

// C-compatible API for macOS Virtualization.framework
// Called from Rust via FFI

@_cdecl("mowis_start_vm")
func mowisStartVM(
    imagePath: UnsafePointer<CChar>,
    memoryMB: UInt64,
    cpuCount: UInt32,
    socketPath: UnsafeMutablePointer<CChar>,
    socketPathLen: UInt32,
    errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> Bool {
    let imagePathStr = String(cString: imagePath)
    
    do {
        // Create VM configuration
        let config = VZVirtualMachineConfiguration()
        
        // CPU configuration
        config.cpuCount = Int(cpuCount)
        
        // Memory configuration
        config.memorySize = memoryMB * 1024 * 1024
        
        // Boot loader (Linux kernel)
        // Note: This is simplified - real implementation needs kernel/initrd
        let bootloader = VZLinuxBootLoader(kernelURL: URL(fileURLWithPath: imagePathStr))
        config.bootLoader = bootloader
        
        // Storage (disk image)
        let diskURL = URL(fileURLWithPath: imagePathStr)
        let diskAttachment = try VZDiskImageStorageDeviceAttachment(
            url: diskURL,
            readOnly: false
        )
        let disk = VZVirtioBlockDeviceConfiguration(attachment: diskAttachment)
        config.storageDevices = [disk]
        
        // Network (NAT)
        let networkDevice = VZVirtioNetworkDeviceConfiguration()
        networkDevice.attachment = VZNATNetworkDeviceAttachment()
        config.networkDevices = [networkDevice]
        
        // Virtio-vsock for socket communication
        let vsockDevice = VZVirtioSocketDeviceConfiguration()
        config.socketDevices = [vsockDevice]
        
        // Validate configuration
        try config.validate()
        
        // Create and start VM
        let vm = VZVirtualMachine(configuration: config)
        
        // Start VM
        vm.start { result in
            switch result {
            case .success:
                print("VM started successfully")
            case .failure(let error):
                print("VM start failed: \(error)")
            }
        }
        
        // Return socket path (vsock exposed as Unix socket)
        let socketPathStr = "/tmp/mowisai-vsock.sock"
        if let cString = socketPathStr.cString(using: .utf8) {
            let len = min(cString.count, Int(socketPathLen))
            socketPath.update(from: cString, count: len)
        }
        
        return true
        
    } catch {
        let errorMsg = "Failed to start VM: \(error.localizedDescription)"
        if let cString = errorMsg.cString(using: .utf8) {
            let buffer = UnsafeMutablePointer<CChar>.allocate(capacity: cString.count)
            buffer.initialize(from: cString, count: cString.count)
            errorOut.pointee = buffer
        }
        return false
    }
}

@_cdecl("mowis_stop_vm")
func mowisStopVM() -> Bool {
    // TODO: Implement VM stop
    // Need to track VM instance globally
    return true
}

@_cdecl("mowis_create_snapshot")
func mowisCreateSnapshot(
    snapshotPath: UnsafePointer<CChar>,
    errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> Bool {
    // TODO: Implement snapshot creation
    // VZVirtualMachine.saveMachineStateToURL
    return true
}

@_cdecl("mowis_restore_snapshot")
func mowisRestoreSnapshot(
    snapshotPath: UnsafePointer<CChar>,
    errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> Bool {
    // TODO: Implement snapshot restoration
    // VZVirtualMachine.restoreMachineStateFromURL
    return true
}
