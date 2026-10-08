// Diagnostic only: fixed, owned loopback fixture; never changes OS settings.
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>
#include <mstcpip.h>
#include <windows.h>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <array>
#include <memory>
#include "response-parser.h"
#pragma comment(lib, "Ws2_32.lib")

namespace {
using Clock = std::chrono::steady_clock;
constexpr int kWidth = 6, kMaxBatches = 128, kIntervalMs = 50;
constexpr int kOperationMs = 1000, kTotalMs = 30000;
constexpr unsigned kMaxOperationRecords = 20000;
constexpr size_t kMaxResponse = 8192;
const char kRequest[] = "GET /readyz HTTP/1.1\r\nHost: 127.0.0.1:3000\r\nConnection: keep-alive\r\n\r\n";
Clock::time_point started;
unsigned opened = 0, closed = 0, eventsOpened = 0, eventsClosed = 0;
unsigned failures = 0, attempts = 0, successes = 0, operationRecords = 0;
bool randomize = false, exhausted = false;
long long elapsed() { return std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - started).count(); }
void record(unsigned id, const char* stage, int result, int error) {
  if (operationRecords >= kMaxOperationRecords) {
    if (!exhausted) {
      ++failures;
      std::printf("{\"type\":\"failure\",\"id\":%u,\"stage\":\"output-cap\",\"error\":10040,\"ms\":%lld}\n", id, elapsed());
    }
    exhausted = true; return;
  }
  ++operationRecords;
  std::printf("{\"type\":\"operation\",\"id\":%u,\"stage\":\"%s\",\"result\":%d,\"error\":%d,\"ms\":%lld}\n", id, stage, result, error, elapsed());
}
void fail(unsigned id, const char* stage, int error) {
  ++failures;
  std::printf("{\"type\":\"failure\",\"id\":%u,\"stage\":\"%s\",\"error\":%d,\"ms\":%lld}\n", id, stage, error, elapsed());
}
struct Owned {
  unsigned id;
  SOCKET socket = INVALID_SOCKET;
  WSAEVENT event = WSA_INVALID_EVENT;
  WSAEVENT writeEvent = WSA_INVALID_EVENT;
  bool connected = false;
  Clock::time_point connectDeadline{};
  explicit Owned(unsigned value): id(value) {}
  Owned(const Owned&) = delete;
  Owned& operator=(const Owned&) = delete;
  ~Owned() {
    if (socket != INVALID_SOCKET) {
      int result = shutdown(socket, SD_SEND);
      int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(id, "shutdown", result, error);
      // WSAENOTCONN is normal after a rejected connect; retain it, do not mask it.
      if (result == SOCKET_ERROR && connected && error != WSAENOTCONN) fail(id, "shutdown", error);
      result = closesocket(socket);
      error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(id, "closesocket", result, error);
      if (result == 0) ++closed; else fail(id, "closesocket", error);
    }
    if (event != WSA_INVALID_EVENT) {
      BOOL result = WSACloseEvent(event);
      int error = result ? 0 : WSAGetLastError();
      record(id, "event-close", result ? 0 : -1, error);
      if (result) ++eventsClosed; else fail(id, "event-close", error);
    }
    if (writeEvent != WSA_INVALID_EVENT) {
      BOOL result = WSACloseEvent(writeEvent);
      int error = result ? 0 : WSAGetLastError();
      record(id, "write-event-close", result ? 0 : -1, error);
      if (result) ++eventsClosed; else fail(id, "write-event-close", error);
    }
  }
};
bool withinBudget(unsigned id) {
  if (exhausted) return false;
  if (elapsed() < kTotalMs) return true;
  if (!exhausted) fail(id, "total-deadline", WSAETIMEDOUT);
  exhausted = true;
  return false;
}
void bindingState(Owned& item, const char* stage) {
  sockaddr_in local{}; int length = sizeof(local);
  int result = getsockname(item.socket, reinterpret_cast<sockaddr*>(&local), &length);
  int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  // Export only whether a local port is assigned, never the address or port.
  record(item.id, stage, result == SOCKET_ERROR ? -1 : local.sin_port ? 1 : 0, error);
}
bool connectStart(Owned& item) {
  ++attempts;
  item.socket = WSASocketW(AF_INET, SOCK_STREAM, IPPROTO_TCP, nullptr, 0, WSA_FLAG_OVERLAPPED);
  int error = item.socket == INVALID_SOCKET ? WSAGetLastError() : 0;
  record(item.id, "socket", item.socket == INVALID_SOCKET ? -1 : 0, error);
  if (item.socket == INVALID_SOCKET) { fail(item.id, "socket", error); return false; }
  ++opened;
  u_long nonblocking = 1;
  int result = ioctlsocket(item.socket, FIONBIO, &nonblocking);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "nonblocking", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "nonblocking", error); return false; }
  BOOL noDelay = TRUE;
  result = setsockopt(item.socket, IPPROTO_TCP, TCP_NODELAY, reinterpret_cast<const char*>(&noDelay), sizeof(noDelay));
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "nodelay", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "nodelay", error); return false; }
  tcp_keepalive keepalive{1, 45000, 45000}; DWORD returned = 0;
  result = WSAIoctl(item.socket, SIO_KEEPALIVE_VALS, &keepalive, sizeof(keepalive), nullptr, 0, &returned, nullptr, nullptr);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "keepalive", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "keepalive", error); return false; }
  item.event = WSACreateEvent();
  error = item.event == WSA_INVALID_EVENT ? WSAGetLastError() : 0;
  record(item.id, "event-create", item.event == WSA_INVALID_EVENT ? -1 : 0, error);
  if (item.event == WSA_INVALID_EVENT) { fail(item.id, "event-create", error); return false; }
  ++eventsOpened;
  // Default Chromium CoreImpl also creates a separate overlapped-write event.
  item.writeEvent = WSACreateEvent();
  error = item.writeEvent == WSA_INVALID_EVENT ? WSAGetLastError() : 0;
  record(item.id, "write-event-create", item.writeEvent == WSA_INVALID_EVENT ? -1 : 0, error);
  if (item.writeEvent == WSA_INVALID_EVENT) { fail(item.id, "write-event-create", error); return false; }
  ++eventsOpened;
  result = WSAEventSelect(item.socket, item.event, FD_CONNECT);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "event-select-connect", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "event-select-connect", error); return false; }
  BOOL value = randomize ? TRUE : FALSE;
  result = setsockopt(item.socket, SOL_SOCKET, SO_RANDOMIZE_PORT, reinterpret_cast<const char*>(&value), sizeof(value));
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "set-randomize", result, error);
  if (result == SOCKET_ERROR) { fail(item.id, "set-randomize", error); return false; }
  BOOL actual = FALSE; int length = sizeof(actual);
  result = getsockopt(item.socket, SOL_SOCKET, SO_RANDOMIZE_PORT, reinterpret_cast<char*>(&actual), &length);
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "get-randomize", result, error);
  if (!exhausted) {
    // Only the bounded Boolean and returned byte length leave this process.
    if (result == SOCKET_ERROR) {
      std::printf("{\"type\":\"option\",\"id\":%u,\"value\":null,\"length\":null,\"ms\":%lld}\n", item.id, elapsed());
    } else {
      std::printf("{\"type\":\"option\",\"id\":%u,\"value\":%s,\"length\":%d,\"ms\":%lld}\n", item.id, actual ? "true" : "false", length >= 0 && length <= 65535 ? length : -1, elapsed());
    }
  }
  if (result == SOCKET_ERROR || length != static_cast<int>(sizeof(actual)) || !!actual != randomize) {
    fail(item.id, "verify-randomize", error ? error : WSAEINVAL); return false;
  }
  sockaddr_in address{}; address.sin_family = AF_INET; address.sin_port = htons(3000);
  address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  // No bind: Chromium's client connect path requests implicit local binding.
  bindingState(item, "binding-before");
  item.connectDeadline = Clock::now() + std::chrono::milliseconds(kOperationMs);
  result = connect(item.socket, reinterpret_cast<sockaddr*>(&address), sizeof(address));
  error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "connect", result, error);
  bindingState(item, "binding-after");
  if (result == 0) { item.connected = true; return true; }
  if (error != WSAEWOULDBLOCK) { fail(item.id, "connect-sync", error); return false; }
  return true;
}
bool connectFinish(Owned& item) {
  if (item.connected) return true;
  long long remaining = kTotalMs - elapsed();
  if (remaining <= 0) { withinBudget(item.id); return false; }
  auto socketRemaining = std::chrono::duration_cast<std::chrono::milliseconds>(item.connectDeadline - Clock::now()).count();
  if (socketRemaining < 0) socketRemaining = 0;
  DWORD wait = static_cast<DWORD>(remaining < socketRemaining ? remaining : socketRemaining);
  DWORD result = WSAWaitForMultipleEvents(1, &item.event, FALSE, wait, FALSE);
  int error = result == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
  record(item.id, "connect-wait", static_cast<int>(result), error);
  if (Clock::now() > item.connectDeadline || !withinBudget(item.id)) { fail(item.id, "connect-wait", WSAETIMEDOUT); return false; }
  if (result != WSA_WAIT_EVENT_0) { fail(item.id, "connect-wait", error ? error : WSAETIMEDOUT); return false; }
  WSANETWORKEVENTS events{};
  int enumerated = WSAEnumNetworkEvents(item.socket, item.event, &events);
  error = enumerated == SOCKET_ERROR ? WSAGetLastError() : 0;
  record(item.id, "connect-enumerate", enumerated, error);
  if (enumerated == SOCKET_ERROR) { fail(item.id, "connect-enumerate", error); return false; }
  if (!(events.lNetworkEvents & FD_CONNECT)) { fail(item.id, "connect-event-missing", WSAEINVAL); return false; }
  error = events.iErrorCode[FD_CONNECT_BIT];
  record(item.id, "connect-async", error ? -1 : 0, error);
  if (error) { fail(item.id, "connect-async", error); return false; }
  if (Clock::now() > item.connectDeadline || !withinBudget(item.id)) { fail(item.id, "connect-wait", WSAETIMEDOUT); return false; }
  item.connected = true; return true;
}
bool ready(SOCKET socket, bool writing, Clock::time_point deadline, unsigned id) {
  while (Clock::now() < deadline && withinBudget(id)) {
    fd_set reads, writes; FD_ZERO(&reads); FD_ZERO(&writes);
    if (writing) FD_SET(socket, &writes); else FD_SET(socket, &reads);
    auto remaining = std::chrono::duration_cast<std::chrono::microseconds>(deadline - Clock::now()).count();
    auto globalRemaining = static_cast<long long>(kTotalMs - elapsed()) * 1000;
    if (globalRemaining < remaining) remaining = globalRemaining;
    if (remaining <= 0) break;
    timeval timeout{0, static_cast<long>(remaining < 10000 ? remaining : 10000)};
    int result = select(0, writing ? nullptr : &reads, writing ? &writes : nullptr, nullptr, &timeout);
    int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    if (result == SOCKET_ERROR) { record(id, "io-select", result, error); fail(id, "io-select", error); return false; }
    if (result > 0 && Clock::now() < deadline && withinBudget(id)) return true;
  }
  fail(id, "http-deadline", WSAETIMEDOUT); return false;
}
bool http(Owned& item) {
  // Simplified post-connect qualification, not Chromium's read/write implementation.
  auto deadline = Clock::now() + std::chrono::milliseconds(kOperationMs);
  size_t sent = 0;
  while (sent < sizeof(kRequest) - 1) {
    if (!ready(item.socket, true, deadline, item.id)) return false;
    int result = send(item.socket, kRequest + sent, static_cast<int>(sizeof(kRequest) - 1 - sent), 0);
    int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    record(item.id, "http-send", result, error);
    if (result <= 0) { fail(item.id, "http-send", error); return false; }
    sent += static_cast<size_t>(result);
  }
  std::string response;
  while (response.size() < kMaxResponse) {
    if (!ready(item.socket, false, deadline, item.id)) return false;
    char buffer[1024];
    size_t capacity = kMaxResponse - response.size();
    if (capacity > sizeof(buffer)) capacity = sizeof(buffer);
    int result = recv(item.socket, buffer, static_cast<int>(capacity), 0);
    int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    record(item.id, "http-recv", result, error);
    if (result <= 0) { fail(item.id, "http-recv", error); return false; }
    response.append(buffer, static_cast<size_t>(result));
    auto parsed = owned_probe::parse_response(response);
    if (parsed == owned_probe::Response::pending) continue;
    if (parsed != owned_probe::Response::complete) { fail(item.id, "fixture-response", WSAEINVAL); return false; }
    if (Clock::now() >= deadline || !withinBudget(item.id)) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
    ++successes; record(item.id, "http-complete", 0, 0); return true;
  }
  fail(item.id, "response-cap", WSAEMSGSIZE); return false;
}
}
int run(int argc, char** argv) {
  if (argc != 3 || (std::strcmp(argv[1], "plain") && std::strcmp(argv[1], "randomized"))) return 2;
  char* end = nullptr; long batches = std::strtol(argv[2], &end, 10);
  if (!argv[2][0] || *end || batches < 1 || batches > kMaxBatches) return 2;
  randomize = !std::strcmp(argv[1], "randomized"); started = Clock::now();
  WSADATA data{}; int startup = WSAStartup(MAKEWORD(2, 2), &data);
  if (startup) return 3;
  struct WinsockScope { bool active = true; ~WinsockScope() { if (active) WSACleanup(); } } winsock;
  std::printf("{\"type\":\"header\",\"schema\":1,\"mode\":\"%s\",\"batches\":%ld,\"width\":6,\"interval_ms\":50,\"total_cap_ms\":30000,\"request\":\"owned-readyz\"}\n", argv[1], batches);
  for (int batch = 0; batch < batches && withinBudget(attempts); ++batch) {
    // Fixed start offsets; no adaptive backoff, failed-request retry or resubmission.
    auto due = started + std::chrono::milliseconds(batch * kIntervalMs);
    auto remaining = std::chrono::duration_cast<std::chrono::microseconds>(due - Clock::now()).count();
    while (remaining > 0) {
      Sleep(static_cast<DWORD>((remaining + 999) / 1000));
      remaining = std::chrono::duration_cast<std::chrono::microseconds>(due - Clock::now()).count();
    }
    std::array<std::unique_ptr<Owned>, kWidth> active;
    for (int lane = 0; lane < kWidth && withinBudget(attempts); ++lane) {
      active[lane] = std::make_unique<Owned>(static_cast<unsigned>(batch * kWidth + lane));
      if (!connectStart(*active[lane])) active[lane].reset();
    }
    for (auto& item : active) {
      if (item && withinBudget(item->id) && connectFinish(*item) && withinBudget(item->id)) http(*item);
      item.reset();
    }
  }
  int cleanup = WSACleanup(); int error = cleanup == SOCKET_ERROR ? WSAGetLastError() : 0;
  winsock.active = false;
  record(attempts, "wsa-cleanup", cleanup, error);
  if (cleanup == SOCKET_ERROR) fail(attempts, "wsa-cleanup", error);
  if (opened != closed || eventsOpened != eventsClosed) fail(attempts, "ownership", WSAEINVAL);
  if (elapsed() > kTotalMs && !exhausted) withinBudget(attempts);
  bool complete = attempts == static_cast<unsigned>(batches * kWidth) && !exhausted && elapsed() <= kTotalMs;
  std::printf("{\"type\":\"summary\",\"attempts\":%u,\"successes\":%u,\"failures\":%u,\"sockets_opened\":%u,\"sockets_closed\":%u,\"events_opened\":%u,\"events_closed\":%u,\"operation_records\":%u,\"complete\":%s,\"elapsed_ms\":%lld}\n", attempts, successes, failures, opened, closed, eventsOpened, eventsClosed, operationRecords, complete ? "true" : "false", elapsed());
  return failures || !complete ? 1 : 0;
}
int main(int argc, char** argv) {
  try { return run(argc, argv); }
  catch (...) {
    // Per-socket and Winsock RAII run first. Missing summary remains a hard failure.
    std::fputs("Native probe exception; evidence is incomplete.\n", stderr);
    return 4;
  }
}
