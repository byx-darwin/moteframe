// Verify the SDK v0.3.6 FFF0 peripheral profile from a Mac.
// swift ble_sdk_compat.swift 802x_Ble_Periphreal | FR29-GAT2 | FR29-RTOS
import CoreBluetooth
import Foundation

final class SDKCheck: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    let name: String
    let serviceID = CBUUID(string: "FFF0")
    let valueID = CBUUID(string: "FFF3")
    let otherID = CBUUID(string: "0783B03E-8535-B5A0-7140-A304F4FF5CBA")
    let readValue = Data([0, 1, 2, 3, 4])
    let notice = Data([1, 2, 3, 4, 5])
    var manager: CBCentralManager!
    var peer: CBPeripheral?
    var value: CBCharacteristic?
    enum Stage { case scan, discover, otherRead, initialRead, subscribe, write, finalRead, done }
    var stage = Stage.scan
    var subscriptionReady = false
    var receivedNotice = false

    init(name: String) {
        self.name = name
        super.init()
        manager = CBCentralManager(delegate: self, queue: .main)
    }

    func fail(_ message: String) -> Never {
        print("FAIL \(stage): \(message)")
        if let peer { manager.cancelPeripheralConnection(peer) }
        exit(1)
    }
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        if central.state == .poweredOn {
            central.scanForPeripherals(withServices: nil)
        } else if central.state != .unknown && central.state != .resetting {
            fail("Bluetooth state \(central.state.rawValue)")
        }
    }
    func centralManager(_ central: CBCentralManager, didDiscover p: CBPeripheral,
                        advertisementData a: [String: Any], rssi: NSNumber) {
        let marker = name == "FR29-GAT2" ? "MFR29A1" : "MFR29R1"
        let marked = name != "802x_Ble_Periphreal" &&
            a[CBAdvertisementDataManufacturerDataKey] as? Data == Data([255, 255] + marker.utf8)
        guard stage == .scan, a[CBAdvertisementDataLocalNameKey] as? String == name || marked else { return }
        peer = p
        p.delegate = self
        stage = .discover
        central.stopScan()
        central.connect(p)
    }
    func centralManager(_ central: CBCentralManager, didConnect p: CBPeripheral) {
        p.discoverServices(nil)
    }
    func centralManager(_ central: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) {
        fail("Connection failed \(String(describing: error))")
    }
    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        guard error == nil, let service = p.services?.first(where: { $0.uuid == serviceID }) else {
            fail("Missing FFF0")
        }
        p.discoverCharacteristics([valueID, otherID], for: service)
    }
    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor s: CBService, error: Error?) {
        guard error == nil, let chars = s.characteristics,
              let first = chars.first(where: { $0.uuid == valueID }),
              first.properties.contains([.read, .write, .notify]),
              let other = chars.first(where: { $0.uuid == otherID }),
              other.properties == .read else {
            let found = (s.characteristics ?? []).map { "\($0.uuid.uuidString)/\($0.properties.rawValue)" }
            fail("SDK discovery on \(s.uuid): \(found), error \(String(describing: error))")
        }
        value = first
        stage = .otherRead
        p.readValue(for: other)
    }
    func peripheral(_ p: CBPeripheral, didUpdateValueFor ch: CBCharacteristic, error: Error?) {
        guard error == nil, let bytes = ch.value, let value else { fail("GATT read or Notify failed") }
        switch stage {
        case .otherRead where ch.uuid == otherID && bytes == Data([0x11, 0x22, 0x33, 0x44, 0x55]):
            stage = .initialRead
            p.readValue(for: value)
        case .initialRead where ch.uuid == valueID && bytes == readValue:
            stage = .subscribe
            p.setNotifyValue(true, for: value)
        case .subscribe where ch.uuid == valueID && bytes == notice:
            receivedNotice = true
            writeWhenReady(p)
        case .finalRead where ch.uuid == valueID && bytes == readValue:
            stage = .done
            manager.cancelPeripheralConnection(p)
        default:
            fail("Unexpected value at \(ch.uuid): \(bytes as NSData)")
        }
    }
    func peripheral(_ p: CBPeripheral, didUpdateNotificationStateFor ch: CBCharacteristic, error: Error?) {
        guard stage == .subscribe, error == nil, ch.uuid == valueID, ch.isNotifying else {
            fail("Subscription failed")
        }
        subscriptionReady = true
        writeWhenReady(p)
    }
    func writeWhenReady(_ p: CBPeripheral) {
        guard stage == .subscribe, receivedNotice, subscriptionReady, let value else { return }
        stage = .write
        p.writeValue(Data(0..<20), for: value, type: .withResponse)
    }
    func peripheral(_ p: CBPeripheral, didWriteValueFor ch: CBCharacteristic, error: Error?) {
        guard stage == .write, error == nil, ch.uuid == valueID else { fail("Write failed") }
        stage = .finalRead
        p.readValue(for: ch)
    }
    func centralManager(_ central: CBCentralManager, didDisconnectPeripheral p: CBPeripheral, error: Error?) {
        guard stage == .done && error == nil else { fail("Disconnected during test") }
        print("PASS \(name) SDK read-only value, FFF3 read, subscription notification, 20-byte write, fixed readback")
        exit(0)
    }
}

guard CommandLine.arguments.count == 2,
      ["802x_Ble_Periphreal", "FR29-GAT2", "FR29-RTOS"].contains(CommandLine.arguments[1]) else {
    print("Usage: ble_sdk_compat.swift 802x_Ble_Periphreal|FR29-GAT2|FR29-RTOS")
    exit(2)
}
let check = SDKCheck(name: CommandLine.arguments[1])
DispatchQueue.main.asyncAfter(deadline: .now() + 40) { check.fail("Timeout") }
RunLoop.main.run()
