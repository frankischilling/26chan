#pragma once
#include <string>
namespace owned_probe {
enum class Response { pending, complete, invalid, oversized };
inline Response parse_response(const std::string& response) {
  constexpr size_t cap = 8192;
  const std::string expected = "synthetic fixture renderer";
  if (response.size() > cap) return Response::oversized;
  size_t boundary = response.find("\r\n\r\n");
  if (boundary == std::string::npos) return response.size() == cap ? Response::oversized : Response::pending;
  if (response.compare(0, 13, "HTTP/1.1 200 ") != 0) return Response::invalid;
  std::string header = response.substr(0, boundary) + "\r\n";
  for (char& c : header) if (c >= 'A' && c <= 'Z') c = static_cast<char>(c + ('a' - 'A'));
  std::string marker = "\r\ncontent-length: " + std::to_string(expected.size()) + "\r\n";
  auto position = header.find(marker);
  if (position == std::string::npos || header.find("\r\ncontent-length:") != position || header.find("\r\ncontent-length:", position + 2) != std::string::npos ||
      header.find("\r\ntransfer-encoding:") != std::string::npos) return Response::invalid;
  if (response.size() < boundary + 4 + expected.size()) return Response::pending;
  return response.substr(boundary + 4) == expected ? Response::complete : Response::invalid;
}
}
