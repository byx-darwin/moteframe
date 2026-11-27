// FR8029D MIC1 acquisition acceptance client. Build with swiftc on the test Mac.
import Foundation
import CoreBluetooth

private let service = CBUUID(string: "2f8029d0-7ff7-48b0-a64e-d6ebc735b251")
private let characteristic = CBUUID(string: "2f8029d1-7ff7-48b0-a64e-d6ebc735b251")

private final class MicCentral: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    private let name: String
    private let verifySaved: Bool
    private let duration: TimeInterval
    private var central: CBCentralManager!
    private var peer: CBPeripheral?
    private var channel: CBCharacteristic?
    private var command: UInt8 = 0
    private var acknowledged = false
    private var response: [UInt8]?
    private let strides: [UInt8] = [6, 12, 8]
    private var change = 0
    private var batches = 0
    private var received = 0
    private var nextSequence: UInt32 = 0
    private var gaps: UInt32 = 0
    private var minimum = UInt16.max
    private var maximum = UInt16.min
    private var stopping = false
    private var stopped = false
    private var total: UInt32 = 0
    private var started = ProcessInfo.processInfo.systemUptime

    init(name: String, verifySaved: Bool, seconds: Int) {
        duration = TimeInterval(seconds)
        self.verifySaved = verifySaved
        self.name = name
        super.init()
        central = CBCentralManager(delegate: self, queue: .main)
        DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(60 + seconds)) { self.fail("timeout") }
    }
    func fail(_ detail: String) -> Never {
        print("FAIL \(detail)")
        exit(1)
    }
    func centralManagerDidUpdateState(_ manager: CBCentralManager) {
        if manager.state == .poweredOn { manager.scanForPeripherals(withServices: nil) }
        else if [.unauthorized,.unsupported,.poweredOff].contains(manager.state) { fail("Bluetooth unavailable \(manager.state.rawValue)") }
    }
    func centralManager(_ manager: CBCentralManager, didDiscover device: CBPeripheral,
                        advertisementData: [String:Any], rssi RSSI: NSNumber) {
        let advertised = advertisementData[CBAdvertisementDataLocalNameKey] as? String
        let marker = advertisementData[CBAdvertisementDataManufacturerDataKey] as? Data
        let knownMarker = name == "FR29-GAT2" ? "MFR29A1" : (name == "FR29-RTOS" ? "MFR29R1" : "")
        let markerMatches = !knownMarker.isEmpty && marker == Data([0xff, 0xff] + Array(knownMarker.utf8))
        guard peer == nil, advertised == name || markerMatches else { return }
        print("CONNECT name=\(name) rssi=\(RSSI)")
        peer = device
        device.delegate = self
        manager.stopScan()
        manager.connect(device)
    }
    func centralManager(_ manager: CBCentralManager, didConnect device: CBPeripheral) { device.discoverServices([service]) }
    func centralManager(_ manager: CBCentralManager, didFailToConnect device: CBPeripheral, error: Error?) { fail("connect \(String(describing:error))") }
    func centralManager(_ manager: CBCentralManager, didDisconnectPeripheral device: CBPeripheral, error: Error?) {
        if stopped && error == nil { print("MIC1_PASS"); exit(0) }
        fail("unexpected disconnect op=\(command) batches=\(batches) received=\(received) gaps=\(gaps) elapsed=\(ProcessInfo.processInfo.systemUptime - started) \(String(describing:error))")
    }
    func peripheral(_ device: CBPeripheral, didDiscoverServices error: Error?) {
        guard error == nil, let found = device.services?.first(where: {$0.uuid == service}) else { fail("service discovery") }
        device.discoverCharacteristics([characteristic], for: found)
    }
    func peripheral(_ device: CBPeripheral, didDiscoverCharacteristicsFor svc: CBService, error: Error?) {
        guard error == nil, let found = svc.characteristics?.first(where: {$0.uuid == characteristic}) else { fail("characteristic discovery") }
        channel = found
        device.setNotifyValue(true, for: found)
    }
    func peripheral(_ device: CBPeripheral, didUpdateNotificationStateFor ch: CBCharacteristic, error: Error?) {
        guard ch.isNotifying && error == nil else { fail("subscription") }
        send(verifySaved ? 5 : 1)
    }
    private func send(_ op: UInt8, _ arg: UInt8 = 0) {
        guard let peer, let channel else { fail("lost channel") }
        command = op
        response = nil
        acknowledged = false
        peer.writeValue(Data([0xad, op, arg, 0]), for: channel, type: .withResponse)
    }
    func peripheral(_ device: CBPeripheral, didWriteValueFor ch: CBCharacteristic, error: Error?) {
        if let error { fail("write op=\(command) \(error)") }
        acknowledged = true
        advance()
    }
    func peripheral(_ device: CBPeripheral, didUpdateValueFor ch: CBCharacteristic, error: Error?) {
        guard error == nil, let value = ch.value, value.count == 20, value[0] == 0xad, value[1] == command else { fail("unexpected Notify") }
        guard response == nil else { fail("duplicate Notify") }
        response = Array(value)
        advance()
    }
    private func advance() {
        guard acknowledged, let b = response else { return }
        func u16(_ i:Int)->UInt16 { UInt16(b[i]) | UInt16(b[i+1]) << 8 }
        func u32(_ i:Int)->UInt32 { UInt32(u16(i)) | UInt32(u16(i+2)) << 16 }
        switch command {
        case 1:
            guard b[2]&1 == 1 else { fail("ADC start") }
            started = ProcessInfo.processInfo.systemUptime
            send(4, strides[change])
        case 4:
            guard b[2]&3 == 3, u16(18) == UInt16(strides[change]) else { fail("expected config active and pending") }
            print("CONFIG active=\(strides[change]) pending=true")
            change += 1
            if change < strides.count { send(4, strides[change]) } else { send(2) }
        case 2:
            let count = Int(b[3])
            guard count <= 5 else { fail("frame length") }
            if count > 0 {
                let first = u32(4)
                guard first >= nextSequence else { fail("repeated or out-of-order sample sequence") }
                gaps += first - nextSequence
                nextSequence = first + UInt32(count)
                for i in 0..<count {
                    let sample = u16(10+i*2)
                    minimum = min(minimum, sample)
                    maximum = max(maximum, sample)
                }
                received += count
            }
            batches += 1
            if stopping {
                if count > 0 { send(2) } else { send(5) }
            } else if ProcessInfo.processInfo.systemUptime - started >= duration {
                send(3)
            } else { send(2) }
        case 3:
            guard b[2]&3 == 2, u16(18) == 8 else { fail("config should still be pending at STOP") }
            stopping = true
            send(2)
        case 5:
            if verifySaved {
                guard b[2]&3 == 0, u16(18) == 8 else { fail("saved config state flags=\(b[2]) stride=\(u16(18))") }
                guard u32(4) == 0, u32(8) == 0, b[3] == 0 else {
                    fail("expected a fresh boot before checking persistence")
                }
                print("MIC1_SAVED stride=8 pending=false raw=0 produced=0")
                stopped = true
                if let peer { central.cancelPeripheralConnection(peer) }
                return
            }
            total = u32(8)
            print("MIC1 raw=\(u32(4)) produced=\(total) received=\(received) dropped=\(u32(12)) fifo_full_observations=\(u16(16)) gaps=\(gaps) batches=\(batches) min=\(minimum) max=\(maximum) elapsed=\(ProcessInfo.processInfo.systemUptime - started)")
            guard b[2]&1 == 0, total > 0, received == Int(total), u32(12) == 0,
                  u16(16) == 0, gaps == 0 else { fail("loss or empty capture") }
            stopped = true
            if let peer { central.cancelPeripheralConnection(peer) }
        default: fail("invalid command")
        }
    }
}

let options = Array(CommandLine.arguments.dropFirst(2))
let verifySaved = options == ["--saved"]
let seconds = options.count == 2 && options[0] == "--seconds" ? Int(options[1]) : 8
guard CommandLine.arguments.count >= 2, let seconds, (1...600).contains(seconds),
      options.isEmpty || verifySaved || (options.count == 2 && options[0] == "--seconds") else {
    print("usage: ble_mic1 DEVICE-NAME [--saved | --seconds 1...600]")
    exit(2)
}
private let client = MicCentral(name: CommandLine.arguments[1], verifySaved: verifySaved, seconds: seconds)
RunLoop.main.run()
