#pragma once
#include <cstddef>
#include <string>
namespace owned_probe {
enum class Response { pending, complete, invalid, oversized };
constexpr std::size_t response_cap = 8192;
inline std::string expected_body(bool streaming) {
  return streaming ? std::string(1024, 'P') + std::string(3072, 'S') : "synthetic fixture renderer";
}
inline bool token_char(unsigned char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
    (c >= '0' && c <= '9') || std::string("!#$%&'*+-.^_`|~").find(static_cast<char>(c)) != std::string::npos;
}
inline std::size_t body_bytes(const std::string& response) {
  const auto boundary = response.find("\r\n\r\n");
  return boundary == std::string::npos ? 0 : response.size() - boundary - 4;
}
// EOF is a framing event: an unfinished response cannot become a successful
// exchange simply because the peer closed. The caller may also reject pending
// explicitly when recv returns zero.
inline Response parse_response(const std::string& response, bool streaming = false, bool eof = false) {
  if (response.size() > response_cap) return Response::oversized;
  const auto boundary = response.find("\r\n\r\n");
  const auto partial = [&]() { return eof ? Response::invalid : Response::pending; };
  if (boundary == std::string::npos) {
    if (response.size() == response_cap) return Response::oversized;
    for (std::size_t i = 0; i < response.size(); ++i) {
      if (response[i] == '\n' && (i == 0 || response[i - 1] != '\r')) return Response::invalid;
      if (response[i] == '\r' && i + 1 < response.size() && response[i + 1] != '\n') return Response::invalid;
      if (response[i] == '\0') return Response::invalid;
    }
    return partial();
  }
  const auto first = response.find("\r\n");
  if (first < 13 || first > boundary || response.compare(0, 13, "HTTP/1.1 200 ") != 0) return Response::invalid;
  for (std::size_t i = 13; i < first; ++i) {
    const auto c = static_cast<unsigned char>(response[i]);
    if (c < 32 || c > 126) return Response::invalid;
  }
  bool length_seen = false;
  std::size_t length = 0;
  for (std::size_t start = first + 2; start < boundary; ) {
    const auto end = response.find("\r\n", start);
    if (end == std::string::npos || end > boundary || end == start) return Response::invalid;
    const auto colon = response.find(':', start);
    if (colon == std::string::npos || colon >= end || colon == start) return Response::invalid;
    std::string name = response.substr(start, colon - start);
    for (char& c : name) {
      if (!token_char(static_cast<unsigned char>(c))) return Response::invalid;
      if (c >= 'A' && c <= 'Z') c = static_cast<char>(c + ('a' - 'A'));
    }
    for (std::size_t i = colon + 1; i < end; ++i) {
      const auto c = static_cast<unsigned char>(response[i]);
      if ((c < 32 && c != '\t') || c > 126) return Response::invalid;
    }
    if (name == "transfer-encoding") return Response::invalid;
    if (name == "content-length") {
      if (length_seen) return Response::invalid;
      length_seen = true;
      std::size_t a = colon + 1, b = end;
      while (a < b && (response[a] == ' ' || response[a] == '\t')) ++a;
      while (b > a && (response[b - 1] == ' ' || response[b - 1] == '\t')) --b;
      if (a == b || response[a] == '0') return Response::invalid;
      for (; a < b; ++a) {
        if (response[a] < '0' || response[a] > '9') return Response::invalid;
        length = length * 10 + static_cast<std::size_t>(response[a] - '0');
        if (length > response_cap) return Response::invalid;
      }
    }
    start = end + 2;
  }
  const std::string expected = expected_body(streaming);
  if (!length_seen || length != expected.size() || boundary + 4 + length > response_cap) return Response::invalid;
  const auto count = body_bytes(response);
  if (count > length || response.compare(boundary + 4, count, expected, 0, count) != 0) return Response::invalid;
  return count == length ? Response::complete : partial();
}
}
