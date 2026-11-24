import XCTest
@testable import MyMouseMacCore

final class KeyMappingTests: XCTestCase {
    func testParsesChord() {
        let chord = KeyChord.parse("Cmd+Shift+C")
        XCTAssertEqual(chord, KeyChord(key: "C", command: true, option: false, shift: true, control: false))
    }

    func testRejectsUnknownModifier() {
        XCTAssertNil(KeyChord.parse("Hyper+C"))
    }

    func testMapsKnownKeyNames() {
        XCTAssertNotNil(KeyCode.forName("C"))
        XCTAssertNotNil(KeyCode.forName("enter"))
        XCTAssertNil(KeyCode.forName("NONEXISTENT"))
    }

    func testDisplayMapperClamps() {
        let mapper = DisplayMapper(sourceWidth: 100, sourceHeight: 100, destination: .init(x: 0, y: 0, width: 200, height: 200))
        XCTAssertEqual(mapper.point(x: 50, y: 50), CGPoint(x: 100, y: 100))
        XCTAssertEqual(mapper.point(x: 999, y: -5), CGPoint(x: 200, y: 0))
    }
}
