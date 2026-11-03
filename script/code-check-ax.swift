// The accessibility tree of the process with the given pid, read from outside
// it through AXUIElement, printed as one JSON object. The Code check runs it
// against its own window (src/browser/code_check.rs). macOS answers only a
// process the user trusts for accessibility; otherwise the object records the
// refusal instead of a tree. It never asks for that trust.
//
//   swift script/code-check-ax.swift <pid>
import ApplicationServices
import Foundation

let names: [AXError: String] = [
    .success: "kAXErrorSuccess",
    .failure: "kAXErrorFailure",
    .illegalArgument: "kAXErrorIllegalArgument",
    .invalidUIElement: "kAXErrorInvalidUIElement",
    .cannotComplete: "kAXErrorCannotComplete",
    .attributeUnsupported: "kAXErrorAttributeUnsupported",
    .noValue: "kAXErrorNoValue",
    .apiDisabled: "kAXErrorAPIDisabled",
    .notImplemented: "kAXErrorNotImplemented",
]

func emit(_ object: [String: Any]) -> Never {
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    FileHandle.standardOutput.write(data)
    exit(0)
}

guard CommandLine.arguments.count == 2, let pid = pid_t(CommandLine.arguments[1]) else {
    emit(["error": "usage: code-check-ax.swift <pid>"])
}

func copy(_ element: AXUIElement, _ name: String) -> (AXError, CFTypeRef?) {
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    return (error, value)
}

var count = 0
func node(_ element: AXUIElement, _ depth: Int) -> [String: Any] {
    count += 1
    var fields: [String: Any] = [:]
    for name in [kAXRoleAttribute, kAXTitleAttribute, kAXDescriptionAttribute,
                 kAXIdentifierAttribute, kAXValueAttribute] {
        let (error, value) = copy(element, name as String)
        if error == .success, let value { fields[name as String] = "\(value)" }
    }
    if depth < 40, count < 4000 {
        let (error, value) = copy(element, kAXChildrenAttribute as String)
        if error == .success, let children = value as? [AXUIElement], !children.isEmpty {
            fields["children"] = children.map { node($0, depth + 1) }
        }
    }
    return fields
}

let app = AXUIElementCreateApplication(pid)
let (error, _) = copy(app, kAXRoleAttribute as String)
var report: [String: Any] = [
    "pid": Int(pid),
    "trusted": AXIsProcessTrusted(),
    "error": Int(error.rawValue),
    "error_name": names[error] ?? "unknown",
]
if error == .success {
    report["tree"] = node(app, 0)
    report["nodes"] = count
}
emit(report)
