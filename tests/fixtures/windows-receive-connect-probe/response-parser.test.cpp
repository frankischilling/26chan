#include "response-parser.h"
#include <cassert>
#include <iostream>
int main() {
  using owned_probe::parse_response; using owned_probe::Response;
  for (bool streaming : {false, true}) {
    const auto body = owned_probe::expected_body(streaming);
    const auto headers = "HTTP/1.1 200 OK\r\nContent-Length: " + std::to_string(body.size()) + "\r\n\r\n";
    const auto full = headers + body;
    for (std::size_t i = 0; i < full.size(); ++i) {
      assert(parse_response(full.substr(0, i), streaming) == Response::pending);
      assert(parse_response(full.substr(0, i), streaming, true) == Response::invalid);
    }
    assert(parse_response(full, streaming) == Response::complete);
    assert(parse_response(full, streaming, true) == Response::complete);
    assert(parse_response(full + "x", streaming) == Response::invalid);
    assert(parse_response(headers + std::string(body.size(), 'x'), streaming) == Response::invalid);
    const auto make = [&](const std::string& h) { return "HTTP/1.1 200 OK\r\n" + h + "\r\n\r\n" + body; };
    const auto length = "Content-Length: " + std::to_string(body.size());
    for (const std::string& invalid : {length + "\r\n" + length, length + "\r\ncontent-length: 1", length + "\r\nTransfer-Encoding: chunked", length + "\r\n bad: value", length + "\r\nBad Header: value", length + "\r\nX: bad\nvalue", std::string("Content-Length: 0"), std::string("Content-Length: -1"), std::string("Content-Length: +25"), std::string("Content-Length: 025"), std::string("Content-Length: 999999999999999999999999999999"), std::string("Content-Length: 25, 25"), std::string("X: absent-length")})
      assert(parse_response(make(invalid), streaming) == Response::invalid);
    assert(parse_response(make("cOnTeNt-LeNgTh:\t" + std::to_string(body.size()) + " \t\r\nX-Fixture: yes"), streaming) == Response::complete);
    assert(parse_response("HTTP/1.0 200 OK\r\n" + length + "\r\n\r\n" + body, streaming) == Response::invalid);
    assert(parse_response("HTTP/1.1 500 Error\r\n" + length + "\r\n\r\n" + body, streaming) == Response::invalid);
  }
  assert(owned_probe::body_bytes("HTTP/1.1 200 OK\r\n") == 0);
  assert(owned_probe::body_bytes("HTTP/1.1 200 OK\r\nContent-Length: 4096\r\n\r\n" + std::string(1024, 'P')) == 1024);
  assert(parse_response(std::string(8192, 'x')) == Response::oversized);
  assert(parse_response(std::string(8193, 'x')) == Response::oversized);
  assert(parse_response("HTTP/1.1 200 OK\n") == Response::invalid);
  std::cout << "Strict parser boundaries passed.\n";
}
