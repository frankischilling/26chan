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
constexpr int kExchanges = 6, kCleanupMs = 35000;
constexpr int kWidth = 6, kMaxBatches = 128, kIntervalMs = 50;
constexpr int kOperationMs = 1000, kTotalMs = 30000;
constexpr unsigned kMaxOperationRecords = 20000;
constexpr size_t kMaxResponse = 8192;
const char kRequest[] = "GET /readyz HTTP/1.1\r\nHost: 127.0.0.1:3000\r\nConnection: keep-alive\r\n\r\n";
Clock::time_point started;
unsigned opened = 0, closed = 0, eventsOpened = 0, eventsClosed = 0;
unsigned failures = 0, attempts = 0, successes = 0, operationRecords = 0;
unsigned syncWrites = 0, asyncWrites = 0, pendingAtClose = 0, postCloseSignals = 0;
unsigned retainedWrites = 0, bytesSent = 0, bytesReceived = 0;
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
struct WriteState {
  WSAOVERLAPPED overlapped{};
  std::array<char, sizeof(kRequest) - 1> buffer{};
  WSABUF descriptor{};
  bool pending = false;
};
struct Owned {
  unsigned id;
  SOCKET socket = INVALID_SOCKET;
  WSAEVENT event = WSA_INVALID_EVENT;
  WSAEVENT writeEvent = WSA_INVALID_EVENT;
  bool connected = false, closeAttempted = false, released = false, readsInitialized = false;
  std::unique_ptr<WriteState> write = std::make_unique<WriteState>();
  Clock::time_point connectDeadline{};
  explicit Owned(unsigned value): id(value) {}
  Owned(const Owned&) = delete;
  Owned& operator=(const Owned&) = delete;
  void closeSocket() {
    if (closeAttempted) return;
    closeAttempted = true;
    record(id, "pending-at-close", write->pending ? 1 : 0, 0);
    if (write->pending) { ++pendingAtClose; fail(id, "pending-at-close", WSA_IO_INCOMPLETE); }
    if (socket != INVALID_SOCKET) {
      int result = shutdown(socket, SD_SEND);
      int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(id, "shutdown", result, error);
      // WSAENOTCONN is normal after a rejected connect; retain it, do not mask it.
      if (result == SOCKET_ERROR && connected && error != WSAENOTCONN) fail(id, "shutdown", error);
      result = closesocket(socket);
      error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(id, "closesocket", result, error);
      if (result == 0) { ++closed; socket = INVALID_SOCKET; }
      else { fail(id, "closesocket", error); exhausted = true; }
    }
  }
  void releaseResources() {
    if (released) return;
    released = true;
    if (write->pending) {
      // Only the retained event is used after close, never a recycled socket handle.
      auto remaining = kCleanupMs - elapsed();
      DWORD wait = remaining > 0 ? static_cast<DWORD>(remaining) : 0;
      DWORD result = WSAWaitForMultipleEvents(1, &writeEvent, FALSE, wait, FALSE);
      int error = result == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
      record(id, "post-close-wait", static_cast<int>(result), error);
      if (result == WSA_WAIT_EVENT_0) {
        ++postCloseSignals;
        record(id, "post-close-signal", 0, 0);
      }
      // Notification alone does not record terminal status or bytes. Fail closed
      // and retain every operation that crossed close, even if signaled.
      ++retainedWrites;
      record(id, "write-retained", 1, 0);
      fail(id, "completion-incomplete", WSA_IO_INCOMPLETE);
      exhausted = true;
      (void)write.release(); // Keep OVERLAPPED and buffer alive until process exit.
      writeEvent = WSA_INVALID_EVENT; // Retain the associated event as well.
    }
    if (event != WSA_INVALID_EVENT) {
      BOOL result = WSACloseEvent(event);
      int error = result ? 0 : WSAGetLastError();
      record(id, "event-close", result ? 0 : -1, error);
      if (result) ++eventsClosed; else { fail(id, "event-close", error); exhausted = true; }
    }
    if (writeEvent != WSA_INVALID_EVENT) {
      BOOL result = WSACloseEvent(writeEvent);
      int error = result ? 0 : WSAGetLastError();
      record(id, "write-event-close", result ? 0 : -1, error);
      if (result) ++eventsClosed; else { fail(id, "write-event-close", error); exhausted = true; }
    }
  }
  ~Owned() { closeSocket(); releaseResources(); }
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
DWORD boundedWait(Clock::time_point deadline) {
  auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - Clock::now()).count();
  auto global = kTotalMs - elapsed();
  if (global < remaining) remaining = global;
  return remaining > 0 ? static_cast<DWORD>(remaining) : 0;
}
bool http(Owned& item, int exchange) {
  auto deadline = Clock::now() + std::chrono::milliseconds(kOperationMs);
  record(item.id, "exchange-begin", exchange, 0);
  size_t sent = 0;
  while (sent < sizeof(kRequest) - 1 && withinBudget(item.id)) {
    if (Clock::now() >= deadline) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
    auto& write = *item.write;
    // Reuse only after the previous operation has a terminal result.
    if (write.pending) { fail(item.id, "write-ownership", WSAEINVAL); return false; }
    BOOL reset = WSAResetEvent(item.writeEvent);
    int error = reset ? 0 : WSAGetLastError();
    record(item.id, "write-reset", reset ? 0 : -1, error);
    if (!reset) { fail(item.id, "write-reset", error); return false; }
    write.overlapped = {}; write.overlapped.hEvent = item.writeEvent;
    const size_t length = sizeof(kRequest) - 1 - sent;
    std::memcpy(write.buffer.data(), kRequest + sent, length);
    write.descriptor = { static_cast<ULONG>(length), write.buffer.data() };
    int result = WSASend(item.socket, &write.descriptor, 1, nullptr, 0, &write.overlapped, nullptr);
    error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    record(item.id, "write-submit", result, error);
    if (result == SOCKET_ERROR && error != WSA_IO_PENDING) { fail(item.id, "write-submit", error); return false; }
    const bool asynchronous = result == SOCKET_ERROR;
    write.pending = asynchronous;
    if (asynchronous) {
      DWORD waited = WSAWaitForMultipleEvents(1, &item.writeEvent, FALSE, boundedWait(deadline), FALSE);
      error = waited == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
      record(item.id, "write-wait", static_cast<int>(waited), error);
      if (waited != WSA_WAIT_EVENT_0) { fail(item.id, "write-wait", error ? error : WSAETIMEDOUT); return false; }
    }
    DWORD transferred = 0, flags = 0;
    BOOL completed = WSAGetOverlappedResult(item.socket, &write.overlapped, &transferred, FALSE, &flags);
    error = completed ? 0 : WSAGetLastError();
    // A provider reporting INCOMPLETE still owns the storage, even after a signal.
    if (completed || error != WSA_IO_INCOMPLETE) write.pending = false;
    else write.pending = true;
    record(item.id, asynchronous ? "write-complete-async" : "write-complete-sync",
           completed ? static_cast<int>(transferred) : -1, error);
    if (!completed) { fail(item.id, "write-complete", error); return false; }
    if (asynchronous) ++asyncWrites; else ++syncWrites;
    bytesSent += transferred;
    if (!transferred || transferred > length) { fail(item.id, "write-length", WSAEINVAL); return false; }
    sent += transferred; // Continue only an unsent suffix, never replay a request.
    if (Clock::now() >= deadline || !withinBudget(item.id)) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
  }
  if (!item.readsInitialized) {
    int result = WSAEventSelect(item.socket, item.event, FD_READ | FD_CLOSE);
    int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    record(item.id, "event-select-read-close", result, error);
    if (result == SOCKET_ERROR) { fail(item.id, "event-select-read-close", error); return false; }
    item.readsInitialized = true;
  }
  std::string response;
  while (response.size() < kMaxResponse && withinBudget(item.id)) {
    if (Clock::now() >= deadline) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
    char buffer[1024];
    size_t capacity = kMaxResponse - response.size();
    if (capacity > sizeof(buffer)) capacity = sizeof(buffer);
    int result = recv(item.socket, buffer, static_cast<int>(capacity), 0);
    int error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
    record(item.id, "http-recv", result, error);
    if (result == SOCKET_ERROR && error == WSAEWOULDBLOCK) {
      DWORD waited = WSAWaitForMultipleEvents(1, &item.event, FALSE, boundedWait(deadline), FALSE);
      error = waited == WSA_WAIT_FAILED ? WSAGetLastError() : 0;
      record(item.id, "read-wait", static_cast<int>(waited), error);
      if (waited != WSA_WAIT_EVENT_0) { fail(item.id, "read-wait", error ? error : WSAETIMEDOUT); return false; }
      WSANETWORKEVENTS events{};
      result = WSAEnumNetworkEvents(item.socket, item.event, &events);
      error = result == SOCKET_ERROR ? WSAGetLastError() : 0;
      record(item.id, "read-enumerate", result, error);
      if (result == SOCKET_ERROR) { fail(item.id, "read-enumerate", error); return false; }
      record(item.id, "read-events", static_cast<int>(events.lNetworkEvents), 0);
      if (events.lNetworkEvents & FD_READ) {
        error = events.iErrorCode[FD_READ_BIT];
        record(item.id, "read-event-error", error ? -1 : 0, error);
        if (error) { fail(item.id, "read-event-error", error); return false; }
      }
      if (events.lNetworkEvents & FD_CLOSE) {
        error = events.iErrorCode[FD_CLOSE_BIT];
        record(item.id, "close-event-error", error ? -1 : 0, error);
        // Even a graceful peer close cannot satisfy six keep-alive exchanges.
        fail(item.id, "unexpected-peer-close", error ? error : WSAECONNRESET); return false;
      }
      if (!(events.lNetworkEvents & FD_READ)) { fail(item.id, "read-event-missing", WSAEINVAL); return false; }
      continue;
    }
    if (result <= 0) { fail(item.id, "http-recv", error ? error : WSAECONNRESET); return false; }
    bytesReceived += static_cast<unsigned>(result);
    response.append(buffer, static_cast<size_t>(result));
    auto parsed = owned_probe::parse_response(response);
    if (parsed == owned_probe::Response::pending) continue;
    if (parsed != owned_probe::Response::complete) { fail(item.id, "fixture-response", WSAEINVAL); return false; }
    if (Clock::now() >= deadline || !withinBudget(item.id)) { fail(item.id, "http-deadline", WSAETIMEDOUT); return false; }
    ++successes; record(item.id, "http-complete", exchange, 0); return true;
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
  std::printf("{\"type\":\"header\",\"schema\":2,\"profile\":\"pooled-event-overlapped\",\"exchanges\":6,\"mode\":\"%s\",\"batches\":%ld,\"width\":6,\"interval_ms\":50,\"total_cap_ms\":30000,\"request\":\"owned-readyz\"}\n", argv[1], batches);
  for (int batch = 0; batch < batches && withinBudget(attempts); ++batch) {
    // Fixed start offsets; no adaptive backoff, failed-request retry or resubmission.
    auto due = started + std::chrono::milliseconds(batch * kIntervalMs);
    auto remaining = std::chrono::duration_cast<std::chrono::microseconds>(due - Clock::now()).count();
    while (remaining > 0) {
      Sleep(static_cast<DWORD>((remaining + 999) / 1000));
      remaining = std::chrono::duration_cast<std::chrono::microseconds>(due - Clock::now()).count();
    }
    std::array<std::unique_ptr<Owned>, kWidth> active;
    std::array<bool, kWidth> initiated{};
    for (int lane = 0; lane < kWidth && withinBudget(attempts); ++lane) {
      active[lane] = std::make_unique<Owned>(static_cast<unsigned>(batch * kWidth + lane));
      initiated[lane] = connectStart(*active[lane]);
    }
    std::array<bool, kWidth> usable{};
    for (int lane = 0; lane < kWidth; ++lane) {
      auto& item = active[lane];
      if (!item || !initiated[lane] || !withinBudget(item->id)) continue;
      // A failed setup never enters the HTTP phase.
      if (item->connectDeadline == Clock::time_point{} || !connectFinish(*item)) continue;
      usable[lane] = true;
    }
    for (int exchange = 0; exchange < kExchanges; ++exchange)
      for (int lane = 0; lane < kWidth; ++lane)
        if (usable[lane] && withinBudget(active[lane]->id)) usable[lane] = http(*active[lane], exchange);
    // Close the pool as a group before draining cancellation notifications.
    for (auto& item : active) if (item) item->closeSocket();
    for (auto& item : active) if (item) item->releaseResources();
    for (auto& item : active) item.reset();
  }
  int cleanup = WSACleanup(); int error = cleanup == SOCKET_ERROR ? WSAGetLastError() : 0;
  winsock.active = false;
  record(attempts, "wsa-cleanup", cleanup, error);
  if (cleanup == SOCKET_ERROR) fail(attempts, "wsa-cleanup", error);
  if (opened != closed || eventsOpened != eventsClosed) fail(attempts, "ownership", WSAEINVAL);
  if (elapsed() > kTotalMs && !exhausted) withinBudget(attempts);
  bool complete = attempts == static_cast<unsigned>(batches * kWidth) && !exhausted && elapsed() <= kTotalMs && retainedWrites == 0;
  std::printf("{\"type\":\"summary\",\"attempts\":%u,\"successes\":%u,\"failures\":%u,\"sockets_opened\":%u,\"sockets_closed\":%u,\"events_opened\":%u,\"events_closed\":%u,\"operation_records\":%u,\"complete\":%s,\"elapsed_ms\":%lld,\"sync_writes\":%u,\"async_writes\":%u,\"pending_at_close\":%u,\"post_close_signals\":%u,\"retained_writes\":%u,\"bytes_sent\":%u,\"bytes_received\":%u}\n", attempts, successes, failures, opened, closed, eventsOpened, eventsClosed, operationRecords, complete ? "true" : "false", elapsed(), syncWrites, asyncWrites, pendingAtClose, postCloseSignals, retainedWrites, bytesSent, bytesReceived);
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
