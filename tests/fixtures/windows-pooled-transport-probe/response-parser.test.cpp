#include "response-parser.h"
#include <cassert>
#include <iostream>
int main() {
  using owned_probe::parse_response; using owned_probe::Response;
  const std::string body = "synthetic fixture renderer";
  const std::string prefix = "HTTP/1.1 200 OK\r\ncontent-length: 26\r\n\r\n";
  const std::string complete = prefix + body;
  for (size_t i = 0; i < complete.size(); ++i) assert(parse_response(complete.substr(0, i)) == Response::pending);
  assert(parse_response(complete) == Response::complete);
  assert(parse_response("HTTP/1.1 200 OK\r\nContent-Length: 26\r\ncontent-type: text/plain\r\n\r\n" + body) == Response::complete);
  assert(parse_response(complete + "x") == Response::invalid);
  assert(parse_response(prefix + std::string(26, 'x')) == Response::invalid);
  assert(parse_response("HTTP/1.1 500 Error\r\ncontent-length: 26\r\n\r\n" + body) == Response::invalid);
  assert(parse_response("HTTP/1.1 200 OK\r\ncontent-length: 25\r\n\r\n" + body) == Response::invalid);
  assert(parse_response("HTTP/1.1 200 OK\r\ncontent-length: 26\r\ncontent-length: 26\r\n\r\n" + body) == Response::invalid);
  assert(parse_response("HTTP/1.1 200 OK\r\ncontent-length: 25\r\ncontent-length: 26\r\n\r\n" + body) == Response::invalid);
  assert(parse_response("HTTP/1.1 200 OK\r\ncontent-length: 26\r\ntransfer-encoding: chunked\r\n\r\n" + body) == Response::invalid);
  assert(parse_response(std::string(8192, 'x')) == Response::oversized);
  assert(parse_response(std::string(8193, 'x')) == Response::oversized);
  assert(parse_response(complete + std::string(8193, 'x')) == Response::oversized);
  std::cout << "Response parser assertions passed (all truncation boundaries and rejection cases).\n";
}
