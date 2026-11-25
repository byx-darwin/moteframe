// Lab central: connect only to RUST-LINK; no pairing or product data writes.
import Foundation
import CoreBluetooth

func emit(_ value: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
       let line = String(data: data, encoding: .utf8) {
        print(line); fflush(stdout)
    }
}

let permission = CBManager.authorization
emit(["event": "authorization", "value": permission.rawValue])
if permission != .allowedAlways {
    emit(["event": "blocked", "reason": "Bluetooth authorization is required"]); exit(3)
}

final class LinkCentral: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    var central: CBCentralManager!
    var target: CBPeripheral?
    var connected = false
    var discovered = false
    let serviceID = CBUUID(string: "7B76CDA0-7B53-4E09-A85C-0135939A0001")
    let characteristicID = CBUUID(string: "7B76CDA0-7B53-4E09-A85C-0135939A0002")
    let testInput = Data([0x52,0x55,0x53,0x54,1,0x23,0x45,0x67])
    let testReply = Data([0xa5,0x5a,0x10,0x20,0x30,0x40,0x50,0x60])
    var writeAcknowledged = false
    var replyVerified = false
    var finished = false
    func finishIfVerified() {
        guard !finished, writeAcknowledged, replyVerified else { return }
        finished = true
        emit(["event": "bidirectional_verified", "write_acknowledged": true,
              "distinct_notification_verified": true])
        if let target { central.cancelPeripheralConnection(target) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { exit(0) }
    }
    override init() {
        super.init()
        central = CBCentralManager(delegate: self, queue: .main,
            options: [CBCentralManagerOptionShowPowerAlertKey: false])
        DispatchQueue.main.asyncAfter(deadline: .now() + 60) { [self] in
            central.stopScan()
            if let target { central.cancelPeripheralConnection(target) }
            emit(["event": "complete", "connected": connected, "services_discovered": discovered,
                  "write_acknowledged": writeAcknowledged, "reply_verified": replyVerified])
            exit(writeAcknowledged && replyVerified ? 0 : 4)
        }
    }
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        emit(["event": "bluetooth_state", "value": central.state.rawValue])
        if central.state == .poweredOn { central.scanForPeripherals(withServices: nil) }
    }
    func centralManager(_ central: CBCentralManager, didDiscover peripheral: CBPeripheral,
                        advertisementData: [String: Any], rssi RSSI: NSNumber) {
        guard target == nil,
              advertisementData[CBAdvertisementDataLocalNameKey] as? String == "RUST-LINK",
              advertisementData[CBAdvertisementDataIsConnectable] as? Bool == true else { return }
        target = peripheral; peripheral.delegate = self; central.stopScan()
        emit(["event": "connect_attempt", "name": "RUST-LINK", "rssi": RSSI])
        central.connect(peripheral)
    }
    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        connected = true; emit(["event": "connected", "name": "RUST-LINK"])
        peripheral.discoverServices(nil)
    }
    func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral,
                        error: Error?) {
        emit(["event": "connect_failed", "error": error?.localizedDescription ?? "unknown"])
    }
    func centralManager(_ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral,
                        error: Error?) {
        emit(["event": "disconnected", "error": error?.localizedDescription ?? "none"])
    }
    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        discovered = error == nil && !(peripheral.services ?? []).isEmpty
        emit(["event": "services", "count": peripheral.services?.count ?? 0,
              "error": error?.localizedDescription ?? "none"])
        if let service = peripheral.services?.first(where: { $0.uuid == serviceID }) {
            peripheral.discoverCharacteristics([characteristicID], for: service)
        }
    }
    func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService,
                    error: Error?) {
        emit(["event": "characteristics", "count": service.characteristics?.count ?? 0,
              "error": error?.localizedDescription ?? "none"])
        if let characteristic = service.characteristics?.first(where: { $0.uuid == characteristicID }) {
            peripheral.setNotifyValue(true, for: characteristic)
        }
    }
    func peripheral(_ peripheral: CBPeripheral, didUpdateNotificationStateFor characteristic: CBCharacteristic,
                    error: Error?) {
        guard characteristic.uuid == characteristicID else { return }
        emit(["event": "subscription", "enabled": characteristic.isNotifying,
              "error": error?.localizedDescription ?? "none"])
        if error == nil && characteristic.isNotifying {
            peripheral.writeValue(testInput, for: characteristic, type: .withResponse)
            emit(["event": "write_sent", "hex": testInput.map { String(format: "%02x", $0) }.joined()])
        }
    }
    func peripheral(_ peripheral: CBPeripheral, didWriteValueFor characteristic: CBCharacteristic,
                    error: Error?) {
        guard characteristic.uuid == characteristicID else { return }
        writeAcknowledged = error == nil
        emit(["event": "write_ack", "success": writeAcknowledged,
              "error": error?.localizedDescription ?? "none"])
        finishIfVerified()
    }
    func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic,
                    error: Error?) {
        guard characteristic.uuid == characteristicID else { return }
        replyVerified = error == nil && characteristic.value == testReply
        emit(["event": "notification", "matches_expected": replyVerified,
              "hex": (characteristic.value ?? Data()).map { String(format: "%02x", $0) }.joined(),
              "error": error?.localizedDescription ?? "none"])
        finishIfVerified()
    }
}
let client = LinkCentral()
RunLoop.main.run()
