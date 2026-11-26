// Mac central for the RUST-LINK and FR8029D bench probes.
import Foundation
import CoreBluetooth

func emit(_ value: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
       let line = String(data: data, encoding: .utf8) {
        print(line)
        fflush(stdout)
    }
}

func hex(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
}

struct WriteExchange {
    let request: Data
    let expectedReply: Data
    private(set) var writeAcknowledged = false
    private(set) var replyVerified = false

    var isVerified: Bool { writeAcknowledged && replyVerified }

    mutating func acknowledge() {
        writeAcknowledged = true
    }

    mutating func observe(_ bytes: Data) -> Bool {
        let matches = bytes == expectedReply
        if matches { replyVerified = true }
        return matches
    }
}

enum AdvertisementMatch: String {
    case localName = "local_name"
    case manufacturerData = "manufacturer_data"
}

enum ProbeProfile: String {
    case rustLink = "RUST-LINK"
    case bare = "FR29-GAT2"
    case freeRTOS = "FR29-RTOS"

    private static let bareMarker = Data([0xff, 0xff] + Array("MFR29A1".utf8))
    private static let freeRTOSMarker = Data([0xff, 0xff] + Array("MFR29R1".utf8))

    var rounds: Int { self == .rustLink ? 1 : 2 }

    func matchAdvertisement(localName: String?, manufacturerData: Data?) -> AdvertisementMatch? {
        if self != .rustLink,
           manufacturerData == Self.bareMarker || manufacturerData == Self.freeRTOSMarker {
            let expected = self == .bare ? Self.bareMarker : Self.freeRTOSMarker
            return manufacturerData == expected ? .manufacturerData : nil
        }
        return localName == rawValue ? .localName : nil
    }

    var serviceID: CBUUID {
        CBUUID(string: self == .rustLink ? "7B76CDA0-7B53-4E09-A85C-0135939A0001"
                                         : "2f8029d0-7ff7-48b0-a64e-d6ebc735b251")
    }

    var characteristicID: CBUUID {
        CBUUID(string: self == .rustLink ? "7B76CDA0-7B53-4E09-A85C-0135939A0002"
                                         : "2f8029d1-7ff7-48b0-a64e-d6ebc735b251")
    }

    func exchange(round: Int) -> WriteExchange {
        if self == .rustLink {
            return WriteExchange(
                request: Data([0x52, 0x55, 0x53, 0x54, 1, 0x23, 0x45, 0x67]),
                expectedReply: Data([0xa5, 0x5a, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60])
            )
        }
        let request = Data((round == 1 ? "FR29NT75-0123456789A" : "FR29RC02-abcdefghijZ").utf8)
        return WriteExchange(request: request, expectedReply: request)
    }
}

struct ProbeOptions {
    static let usage = "Usage: ble_connect.swift [RUST-LINK|FR29-GAT2|FR29-RTOS] [--timeout SECONDS]"
    let probe: ProbeProfile
    let timeout: Int

    static func parse(_ arguments: [String]) -> Self? {
        var remaining = arguments
        var timeout = 60
        if let index = remaining.firstIndex(of: "--timeout") {
            guard remaining.count > index + 1,
                  let seconds = Int(remaining[index + 1]),
                  (1...300).contains(seconds) else { return nil }
            timeout = seconds
            remaining.removeSubrange(index...index + 1)
        }
        guard remaining.count <= 1,
              let probe = ProbeProfile(rawValue: remaining.first ?? "RUST-LINK")
        else { return nil }
        return Self(probe: probe, timeout: timeout)
    }
}

final class LinkCentral: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    private enum Phase: String {
        case bluetooth, scan, connect
        case serviceDiscovery = "service_discovery"
        case characteristicDiscovery = "characteristic_discovery"
        case subscription
        case writeResponse = "write_response"
        case notification, verified, disconnect, finished

        var awaitingExchange: Bool { self == .writeResponse || self == .notification }
    }

    private let probe: ProbeProfile
    private let serviceID: CBUUID
    private let characteristicID: CBUUID
    private var central: CBCentralManager!
    private var target: CBPeripheral?
    private var identity: UUID?
    private var phase: Phase = .bluetooth
    private var round = 1
    private var completedRounds = 0
    private var exchange: WriteExchange
    private var connected = false
    private var discovered = false
    private var advertisementReports = 0
    private var matchingAdvertisements = 0

    init(options: ProbeOptions) {
        probe = options.probe
        serviceID = probe.serviceID
        characteristicID = probe.characteristicID
        exchange = probe.exchange(round: 1)
        super.init()
        central = CBCentralManager(delegate: self, queue: .main,
            options: [CBCentralManagerOptionShowPowerAlertKey: false])
        DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(options.timeout)) { [self] in
            fail("timeout")
        }
    }

    private func report(_ event: String, _ fields: [String: Any] = [:]) {
        var value = fields
        value["event"] = event
        value["name"] = probe.rawValue
        value["round"] = round
        emit(value)
    }

    private func scan() {
        phase = .scan
        report("scan_started")
        central.scanForPeripherals(
            withServices: nil,
            options: [CBCentralManagerScanOptionAllowDuplicatesKey: true]
        )
    }

    private func finish(_ status: Int32, reason: String) {
        guard phase != .finished else { return }
        report("complete", ["phase": phase.rawValue, "reason": reason,
                            "connected": connected, "services_discovered": discovered,
                            "write_acknowledged": exchange.writeAcknowledged,
                            "reply_verified": exchange.replyVerified,
                            "completed_rounds": completedRounds,
                            "advertisement_reports": advertisementReports,
                            "matching_advertisements": matchingAdvertisements])
        phase = .finished
        if central.state == .poweredOn {
            central.stopScan()
            if let target { central.cancelPeripheralConnection(target) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { exit(status) }
    }

    private func fail(_ reason: String) {
        guard phase != .finished else { return }
        report("failed", ["phase": phase.rawValue, "reason": reason])
        finish(4, reason: reason)
    }

    private func finishExchange(_ peripheral: CBPeripheral) {
        guard phase.awaitingExchange, exchange.isVerified else { return }
        completedRounds += 1
        let verification = probe == .rustLink ? "distinct_notification_verified" : "echo_notification_verified"
        report("bidirectional_verified", ["write_acknowledged": true, verification: true])

        if probe == .rustLink {
            finish(0, reason: "verified")
            return
        }
        phase = .verified
        // Let the bench SWD observer sample the transmission before disconnecting.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { [self] in
            guard phase == .verified, peripheral === target else { return }
            phase = .disconnect
            report("disconnect_requested")
            central.cancelPeripheralConnection(peripheral)
        }
    }

    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        emit(["event": "bluetooth_state", "value": central.state.rawValue])
        if central.state == .poweredOn && phase == .bluetooth {
            scan()
        } else if central.state == .unauthorized || central.state == .unsupported
                    || (phase != .bluetooth && central.state != .poweredOn) {
            fail("Bluetooth state \(central.state.rawValue)")
        }
    }

    func centralManager(_ central: CBCentralManager, didDiscover peripheral: CBPeripheral,
                        advertisementData: [String: Any], rssi RSSI: NSNumber) {
        guard phase == .scan else { return }
        advertisementReports += 1
        guard let match = probe.matchAdvertisement(
            localName: advertisementData[CBAdvertisementDataLocalNameKey] as? String,
            manufacturerData: advertisementData[CBAdvertisementDataManufacturerDataKey] as? Data
        ) else { return }
        matchingAdvertisements += 1
        guard advertisementData[CBAdvertisementDataIsConnectable] as? Bool == true,
              identity == nil || identity == peripheral.identifier else { return }

        identity = peripheral.identifier
        target = peripheral
        peripheral.delegate = self
        central.stopScan()
        phase = .connect
        report("connect_attempt", ["rssi": RSSI, "matched_by": match.rawValue])
        central.connect(peripheral)
    }

    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        guard peripheral === target, phase == .connect else { return }
        connected = true
        phase = .serviceDiscovery
        report("connected")
        peripheral.discoverServices(probe == .rustLink ? nil : [serviceID])
    }

    func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral,
                        error: Error?) {
        guard peripheral === target, phase == .connect else { return }
        let reason = error?.localizedDescription ?? "connection failed"
        report("connect_failed", ["error": reason])
        fail(reason)
    }

    func centralManager(_ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral,
                        error: Error?) {
        guard peripheral === target else { return }
        report("disconnected", ["error": error?.localizedDescription ?? "none"])
        guard phase != .finished else { return }
        guard phase == .disconnect, error == nil else {
            fail(error?.localizedDescription ?? "unexpected disconnect")
            return
        }
        target = nil
        if completedRounds == probe.rounds {
            report("reconnect_verified", ["completed_rounds": completedRounds])
            finish(0, reason: "verified")
        } else {
            round += 1
            exchange = probe.exchange(round: round)
            scan()
        }
    }

    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard peripheral === target, phase == .serviceDiscovery else { return }
        let services = peripheral.services ?? []
        report("services", ["count": services.count, "error": error?.localizedDescription ?? "none"])
        guard error == nil, let service = services.first(where: { $0.uuid == serviceID }) else {
            fail(error?.localizedDescription ?? "expected service not found")
            return
        }
        discovered = true
        phase = .characteristicDiscovery
        peripheral.discoverCharacteristics([characteristicID], for: service)
    }

    func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService,
                    error: Error?) {
        guard peripheral === target, phase == .characteristicDiscovery, service.uuid == serviceID
        else { return }
        let characteristics = service.characteristics ?? []
        report("characteristics", ["count": characteristics.count,
                                   "error": error?.localizedDescription ?? "none"])
        guard error == nil,
              let characteristic = characteristics.first(where: { $0.uuid == characteristicID })
        else {
            fail(error?.localizedDescription ?? "expected characteristic not found")
            return
        }
        guard probe == .rustLink || characteristic.properties.contains([.write, .notify]) else {
            fail("expected characteristic does not support Write Request and Notify")
            return
        }
        phase = .subscription
        peripheral.setNotifyValue(true, for: characteristic)
    }

    private func isCurrent(_ peripheral: CBPeripheral, _ characteristic: CBCharacteristic) -> Bool {
        peripheral === target && characteristic.uuid == characteristicID
            && characteristic.service?.uuid == serviceID
    }

    func peripheral(_ peripheral: CBPeripheral, didUpdateNotificationStateFor characteristic: CBCharacteristic,
                    error: Error?) {
        guard phase == .subscription, isCurrent(peripheral, characteristic) else { return }
        report("subscription", ["enabled": characteristic.isNotifying,
                                "error": error?.localizedDescription ?? "none"])
        guard error == nil, characteristic.isNotifying else {
            fail(error?.localizedDescription ?? "Notify subscription was not enabled")
            return
        }
        phase = .writeResponse
        report("write_sent", ["hex": hex(exchange.request)])
        peripheral.writeValue(exchange.request, for: characteristic, type: .withResponse)
    }

    func peripheral(_ peripheral: CBPeripheral, didWriteValueFor characteristic: CBCharacteristic,
                    error: Error?) {
        guard phase == .writeResponse, isCurrent(peripheral, characteristic) else { return }
        report("write_ack", ["success": error == nil, "error": error?.localizedDescription ?? "none"])
        if let error { fail(error.localizedDescription); return }
        exchange.acknowledge()
        phase = .notification
        finishExchange(peripheral)
    }

    func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic,
                    error: Error?) {
        guard phase.awaitingExchange, isCurrent(peripheral, characteristic) else { return }
        if let error { fail(error.localizedDescription); return }
        let bytes = characteristic.value ?? Data()
        let matches = exchange.observe(bytes)
        report("notification", ["matches_expected": matches, "hex": hex(bytes), "error": "none"])
        finishExchange(peripheral)
    }
}

#if BLE_CONNECT_TESTS
func testProbeAdvertisementMatching() {
    let bare = Data([0xff, 0xff, 0x4d, 0x46, 0x52, 0x32, 0x39, 0x41, 0x31])
    let freeRTOS = Data([0xff, 0xff, 0x4d, 0x46, 0x52, 0x32, 0x39, 0x52, 0x31])
    precondition(ProbeProfile.bare.matchAdvertisement(localName: nil, manufacturerData: bare) == .manufacturerData,
                 "a bare-metal report with the bench marker must match without a local name")
    precondition(ProbeProfile.freeRTOS.matchAdvertisement(localName: nil, manufacturerData: freeRTOS) == .manufacturerData)
    precondition(ProbeProfile.freeRTOS.matchAdvertisement(localName: nil, manufacturerData: bare) == nil)
    precondition(ProbeProfile.freeRTOS.matchAdvertisement(localName: "FR29-RTOS", manufacturerData: bare) == nil)
    precondition(ProbeProfile.bare.matchAdvertisement(localName: nil, manufacturerData: Data(bare.dropLast())) == nil)
    precondition(ProbeProfile.bare.matchAdvertisement(localName: "FR29-GAT2", manufacturerData: nil) == .localName)
    precondition(ProbeProfile.rustLink.matchAdvertisement(localName: "RUST-LINK", manufacturerData: nil) == .localName)
    precondition(ProbeProfile.rustLink.matchAdvertisement(localName: nil, manufacturerData: bare) == nil)
}

func testProbeExchanges() {
    precondition(ProbeOptions.parse([])?.probe == .rustLink)
    precondition(ProbeOptions.parse(["FR29-GAT2", "--timeout", "12"])?.timeout == 12)
    precondition(ProbeOptions.parse(["802x_Ble_Periphreal"]) == nil)
    precondition(ProbeOptions.parse(["--timeout", "0"]) == nil)

    var legacy = ProbeProfile.rustLink.exchange(round: 1)
    precondition(legacy.request == Data([0x52, 0x55, 0x53, 0x54, 1, 0x23, 0x45, 0x67]))
    legacy.acknowledge()
    precondition(!legacy.observe(legacy.request) && !legacy.isVerified)
    precondition(legacy.observe(Data([0xa5, 0x5a, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60])))
    precondition(legacy.isVerified)

    for probe in [ProbeProfile.bare, .freeRTOS] {
        var first = probe.exchange(round: 1)
        var second = probe.exchange(round: 2)
        precondition(probe.rounds == 2 && first.request.count == 20 && second.request.count == 20)
        precondition(first.request != second.request)
        precondition(first.observe(first.request) && !first.isVerified)
        first.acknowledge()
        precondition(first.isVerified)
        second.acknowledge()
        precondition(!second.observe(first.request) && !second.isVerified)
        precondition(second.observe(second.request) && second.isVerified)
    }
}
testProbeAdvertisementMatching()
testProbeExchanges()
print("BLE probe advertisement and exchange checks passed")
#else
let arguments = Array(CommandLine.arguments.dropFirst())
if arguments == ["--help"] {
    print(ProbeOptions.usage)
    exit(0)
}
guard let options = ProbeOptions.parse(arguments) else {
    fputs(ProbeOptions.usage + "\n", stderr)
    exit(2)
}
let permission = CBManager.authorization
emit(["event": "authorization", "value": permission.rawValue])
guard permission == .allowedAlways else {
    emit(["event": "blocked", "reason": "Bluetooth authorization is required"])
    exit(3)
}
let client = LinkCentral(options: options)
RunLoop.main.run()
#endif
