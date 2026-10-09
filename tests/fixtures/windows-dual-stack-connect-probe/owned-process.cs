// Native Windows launcher helper. Every child starts suspended inside its own
// kill-on-close Job Object; no child code runs before containment is established.
using System;
using System.ComponentModel;
using System.Collections.Generic;
using System.IO;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public sealed class DualStackOwnedProcess : IDisposable
{
    const uint CREATE_SUSPENDED = 4, CREATE_NO_WINDOW = 0x08000000;
    const uint STARTF_USESTDHANDLES = 0x100, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000;
    IntPtr job;
    Process process;
    public int Id { get { return process.Id; } }
    public DateTime StartTime { get { return process.StartTime; } }
    public bool HasExited { get { return process.HasExited; } }
    public int ExitCode { get { return process.ExitCode; } }

    [StructLayout(LayoutKind.Sequential)] struct SecurityAttributes { public int Length; public IntPtr Descriptor; [MarshalAs(UnmanagedType.Bool)] public bool Inherit; }
    [StructLayout(LayoutKind.Sequential)] struct StartupInfo {
        public uint Size; public IntPtr Reserved, Desktop, Title;
        public uint X, Y, XSize, YSize, XCountChars, YCountChars, FillAttribute, Flags;
        public ushort ShowWindow, Reserved2Size; public IntPtr Reserved2, Input, Output, Error;
    }
    [StructLayout(LayoutKind.Sequential)] struct StartupInfoEx { public StartupInfo Startup; public IntPtr Attributes; }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInformation { public IntPtr Process, Thread; public uint ProcessId, ThreadId; }
    [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
        public long ProcessUserTimeLimit, JobUserTimeLimit; public uint Flags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize; public uint ActiveProcessLimit;
        public UIntPtr Affinity; public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)] struct IoCounters { public ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes; }
    [StructLayout(LayoutKind.Sequential)] struct ExtendedLimits {
        public BasicLimits Basic; public IoCounters Io;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)] struct Accounting {
        public long UserTime, KernelTime, PeriodUserTime, PeriodKernelTime;
        public uint PageFaults, TotalProcesses, ActiveProcesses, TerminatedProcesses;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int informationClass, ref ExtendedLimits info, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int informationClass, out Accounting info, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
    [DllImport("kernel32.dll", EntryPoint = "QueryInformationJobObject", SetLastError = true)] static extern bool QueryJobProcessIds(IntPtr job, int informationClass, IntPtr info, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder path, ref uint size);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern uint GetSystemDirectoryW(StringBuilder path, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateFileW(string path, uint access, uint share, ref SecurityAttributes attributes, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcessW(string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string directory, ref StartupInfoEx startup, out ProcessInformation info);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, uint flags, ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, UIntPtr attribute, IntPtr value, UIntPtr size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
    static void Check(bool value) { if (!value) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static IntPtr Open(string path, bool input) {
        var attributes = new SecurityAttributes { Length = Marshal.SizeOf<SecurityAttributes>(), Inherit = true };
        IntPtr handle = CreateFileW(path, input ? 0x80000000u : 0x40000000u, 3, ref attributes, input ? 3u : 2u, 0x80, IntPtr.Zero);
        Check(handle != new IntPtr(-1)); return handle;
    }
    public static DualStackOwnedProcess Start(string executable, string arguments, string directory, string stdout, string stderr) {
        var child = new DualStackOwnedProcess();
        IntPtr input = IntPtr.Zero, output = IntPtr.Zero, error = IntPtr.Zero;
        IntPtr attributes = IntPtr.Zero, handles = IntPtr.Zero;
        bool attributesReady = false;
        ProcessInformation info = new ProcessInformation();
        bool launched = false, assigned = false;
        try {
            child.job = CreateJobObjectW(IntPtr.Zero, null); Check(child.job != IntPtr.Zero);
            var limits = new ExtendedLimits(); limits.Basic.Flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            Check(SetInformationJobObject(child.job, 9, ref limits, (uint)Marshal.SizeOf<ExtendedLimits>()));
            input = Open("NUL", true); output = Open(stdout, false); error = Open(stderr, false);
            IntPtr size = IntPtr.Zero;
            InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
            Check(size != IntPtr.Zero);
            attributes = Marshal.AllocHGlobal(size);
            Check(InitializeProcThreadAttributeList(attributes, 1, 0, ref size)); attributesReady = true;
            handles = Marshal.AllocHGlobal(IntPtr.Size * 3);
            Marshal.Copy(new[] { input, output, error }, 0, handles, 3);
            // PROC_THREAD_ATTRIBUTE_HANDLE_LIST limits inheritance to these three
            // streams, excluding unrelated inheritable handles in the host.
            Check(UpdateProcThreadAttribute(attributes, 0, new UIntPtr(0x00020002), handles, new UIntPtr((uint)(IntPtr.Size * 3)), IntPtr.Zero, IntPtr.Zero));
            var startup = new StartupInfoEx { Startup = new StartupInfo { Size = (uint)Marshal.SizeOf<StartupInfoEx>(), Flags = STARTF_USESTDHANDLES, Input = input, Output = output, Error = error }, Attributes = attributes };
            Check(CreateProcessW(executable, new StringBuilder("\"" + executable + "\" " + arguments), IntPtr.Zero, IntPtr.Zero, true, CREATE_SUSPENDED | CREATE_NO_WINDOW | 0x00080000, IntPtr.Zero, directory, ref startup, out info));
            launched = true;
            Check(AssignProcessToJobObject(child.job, info.Process)); assigned = true;
            child.process = Process.GetProcessById((int)info.ProcessId);
            // Open/cache the process handle before a short-lived child can exit.
            if (child.process.Handle == IntPtr.Zero) throw new InvalidOperationException("Owned process handle unavailable.");
            Check(ResumeThread(info.Thread) != uint.MaxValue);
            return child;
        } catch {
            if (launched) {
                // Before assignment the suspended process has run no code and has
                // no descendants. After assignment the job owns the whole tree.
                bool stopped = assigned ? TerminateJobObject(child.job, 1) : TerminateProcess(info.Process, 1);
                if (!stopped || WaitForSingleObject(info.Process, 5000) != 0) {
                    child.Dispose(); throw new InvalidOperationException("Owned launch cleanup unverified.");
                }
            }
            child.Dispose(); throw;
        } finally {
            if (attributesReady) DeleteProcThreadAttributeList(attributes);
            if (attributes != IntPtr.Zero) Marshal.FreeHGlobal(attributes);
            if (handles != IntPtr.Zero) Marshal.FreeHGlobal(handles);
            foreach (IntPtr handle in new[] { input, output, error, info.Thread, info.Process }) {
                if (handle != IntPtr.Zero && handle != new IntPtr(-1)) CloseHandle(handle);
            }
        }
    }
    public uint ActiveProcessCount {
        get { Accounting info; Check(QueryInformationJobObject(job, 1, out info, (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero)); return info.ActiveProcesses; }
    }
    public bool TreeExited { get { return ActiveProcessCount == 0; } }
    // Read-only membership proof against this exact owned job, never any job.
    public bool ContainsProcess(Process candidate) {
        bool member; Check(IsProcessInJob(candidate.Handle, job, out member)); return member;
    }
    public sealed class OwnershipSnapshot {
        public bool RootExited, RootInJob, RootMembershipObserved, DescendantExited, DescendantInJob, AccountingLayoutValid;
        public uint ActiveProcesses, TotalProcesses, TerminatedProcesses;
        public bool ProcessListComplete, ProcessCountsConsistent;
        public uint EnumeratedProcesses, RootRoleCount, DescendantRoleCount, ConsoleHostRoleCount, UnknownRoleCount, UnavailableRoleCount;
        public bool RolesQualified(bool rootExpected) {
            return ProcessListComplete && ProcessCountsConsistent && EnumeratedProcesses <= 16 &&
                UnknownRoleCount == 0 && UnavailableRoleCount == 0 &&
                RootRoleCount == (rootExpected ? 1u : 0u) && DescendantRoleCount == 1 &&
                ConsoleHostRoleCount <= 16 &&
                EnumeratedProcesses == RootRoleCount + DescendantRoleCount + ConsoleHostRoleCount &&
                ActiveProcesses == EnumeratedProcesses;
        }
    }
    // Bounded read-only diagnostics. No process IDs, handles or paths are exposed.
    public OwnershipSnapshot InspectOwnership(Process candidate) {
        Accounting info;
        Check(QueryInformationJobObject(job, 1, out info, (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero));
        bool rootExited = process.HasExited;
        bool descendantExited = candidate.HasExited;
        var snapshot = new OwnershipSnapshot {
            RootExited = rootExited,
            RootMembershipObserved = !rootExited,
            RootInJob = !rootExited && ContainsProcess(process),
            DescendantExited = descendantExited,
            DescendantInJob = !descendantExited && ContainsProcess(candidate),
            AccountingLayoutValid = Marshal.SizeOf<Accounting>() == 48 && Marshal.OffsetOf<Accounting>("ActiveProcesses").ToInt64() == 40,
            ActiveProcesses = info.ActiveProcesses,
            TotalProcesses = info.TotalProcesses,
            TerminatedProcesses = info.TerminatedProcesses
        };
        InspectRoles(snapshot, candidate);
        return snapshot;
    }
    public static bool IsExpectedConsoleHost(string image, string systemDirectory) {
        return !String.IsNullOrEmpty(systemDirectory) && String.Equals(image, Path.Combine(systemDirectory, "conhost.exe"), StringComparison.OrdinalIgnoreCase);
    }
    void InspectRoles(OwnershipSnapshot snapshot, Process candidate) {
        // Single fixed-capacity query. A larger or changing job is inconclusive;
        // never resize/retry or enumerate unrelated system processes.
        const int capacity = 16;
        int bytes = 8 + capacity * IntPtr.Size;
        IntPtr buffer = Marshal.AllocHGlobal(bytes);
        try {
            if (!QueryJobProcessIds(job, 3, buffer, (uint)bytes, IntPtr.Zero)) return;
            uint assigned = unchecked((uint)Marshal.ReadInt32(buffer, 0));
            uint listed = unchecked((uint)Marshal.ReadInt32(buffer, 4));
            if (assigned != listed || listed > capacity) return;
            snapshot.ProcessListComplete = true;
            snapshot.EnumeratedProcesses = listed;
            var seen = new HashSet<long>();
            var system = new StringBuilder(1024);
            uint systemLength = GetSystemDirectoryW(system, (uint)system.Capacity);
            string systemDirectory = systemLength > 0 && systemLength < system.Capacity ? system.ToString() : null;
            for (int index = 0; index < listed; index++) {
                long id = Marshal.ReadIntPtr(buffer, 8 + index * IntPtr.Size).ToInt64();
                if (id <= 0 || id > int.MaxValue || !seen.Add(id)) {
                    snapshot.ProcessListComplete = false; snapshot.UnavailableRoleCount++; continue;
                }
                if (id == process.Id) {
                    if (!snapshot.RootExited && snapshot.RootInJob) snapshot.RootRoleCount++;
                    else snapshot.UnavailableRoleCount++;
                    continue;
                }
                if (id == candidate.Id) {
                    if (!snapshot.DescendantExited && snapshot.DescendantInJob) snapshot.DescendantRoleCount++;
                    else snapshot.UnavailableRoleCount++;
                    continue;
                }
                // Query-only plus synchronization rights; this handle is never
                // used to terminate a process. Only the owned job is terminated.
                IntPtr member = OpenProcess(0x00101000, false, (uint)id);
                if (member == IntPtr.Zero) { snapshot.UnavailableRoleCount++; continue; }
                try {
                    bool belongs;
                    if (!IsProcessInJob(member, job, out belongs) || !belongs || WaitForSingleObject(member, 0) != 258) {
                        snapshot.UnavailableRoleCount++; continue;
                    }
                    var image = new StringBuilder(1024);
                    uint length = (uint)image.Capacity;
                    if (systemDirectory == null || !QueryFullProcessImageNameW(member, 0, image, ref length) || length == 0 || length >= image.Capacity) {
                        snapshot.UnavailableRoleCount++; continue;
                    }
                    // Full OS-resolved system path only. A basename or prefix
                    // match must never admit a different image.
                    if (IsExpectedConsoleHost(image.ToString(), systemDirectory)) snapshot.ConsoleHostRoleCount++;
                    else snapshot.UnknownRoleCount++;
                } finally { Check(CloseHandle(member)); }
            }
            Accounting after;
            Check(QueryInformationJobObject(job, 1, out after, (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero));
            snapshot.ProcessCountsConsistent = listed == snapshot.ActiveProcesses && after.ActiveProcesses == snapshot.ActiveProcesses && after.TotalProcesses == snapshot.TotalProcesses && after.TerminatedProcesses == snapshot.TerminatedProcesses;
        } finally { Marshal.FreeHGlobal(buffer); }
    }
    public bool WaitForExit(int milliseconds) {
        var watch = Stopwatch.StartNew();
        if (!process.WaitForExit(milliseconds)) return false;
        while (!TreeExited) {
            if (watch.ElapsedMilliseconds >= milliseconds) return false;
            Thread.Sleep(10);
        }
        return true;
    }
    public void Kill() { Check(TerminateJobObject(job, 1)); }
    public void Kill(bool entireProcessTree) { Kill(); }
    public void Dispose() {
        // The job handle remains owned even if the root has already exited.
        // KILL_ON_JOB_CLOSE contains descendants on every exceptional exit.
        if (job != IntPtr.Zero) { Check(CloseHandle(job)); job = IntPtr.Zero; }
        if (process != null) { process.Dispose(); process = null; }
    }
}
