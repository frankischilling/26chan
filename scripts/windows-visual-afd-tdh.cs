using System;
using System.Globalization;
using System.Runtime.InteropServices;

namespace Paperboard.WindowsAfd
{
    // Metadata enumeration only. This helper does not create or consume a trace.
    // https://learn.microsoft.com/en-us/windows/win32/api/tdh/nf-tdh-tdhenumeratemanifestproviderevents
    // https://learn.microsoft.com/en-us/windows/win32/api/tdh/ns-tdh-provider_event_info
    // https://learn.microsoft.com/en-us/windows/win32/api/evntprov/ns-evntprov-event_descriptor
    public static class TdhMetadata
    {
        public const string ProviderGuid = "e53c6823-7bb8-44bb-90dc-3f86090d48a6";
        public const int MaxBytes = 262144;
        public const int MaxDescriptors = 4096;
        private const int HeaderBytes = 8;
        private const int DescriptorBytes = 16;

        // These structs are layout witnesses for Marshal.SizeOf/OffsetOf only.
        // Their fields are intentionally never assigned by managed code.
#pragma warning disable CS0649
        [StructLayout(LayoutKind.Sequential)]
        private struct EventDescriptor
        {
            public ushort Id;
            public byte Version;
            public byte Channel;
            public byte Level;
            public byte Opcode;
            public ushort Task;
            public ulong Keyword;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProviderEventInfo
        {
            public uint NumberOfEvents;
            public uint Reserved;
            public EventDescriptor FirstDescriptor;
        }

#pragma warning restore CS0649

        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("tdh.dll", ExactSpelling = true, CallingConvention = CallingConvention.Winapi)]
        private static extern uint TdhEnumerateManifestProviderEvents(
            ref Guid providerGuid, IntPtr buffer, ref uint bufferSize);

        public sealed class Descriptor
        {
            public ushort id { get; set; }
            public byte version { get; set; }
            public byte channel { get; set; }
            public byte level { get; set; }
            public byte opcode { get; set; }
            public ushort task { get; set; }
            public string keyword_mask { get; set; }
        }

        public sealed class Result
        {
            public string status { get; set; }
            public string provider_guid { get; set; }
            public string reason { get; set; }
            public uint? api_status { get; set; }
            public Descriptor[] descriptors { get; set; }
        }

        private static Result Unavailable(string reason, uint? apiStatus = null)
        {
            return new Result { status = "unavailable", provider_guid = ProviderGuid,
                reason = reason, api_status = apiStatus, descriptors = new Descriptor[0] };
        }

        private static bool SupportedLayout()
        {
            return BitConverter.IsLittleEndian && Marshal.SizeOf(typeof(EventDescriptor)) == DescriptorBytes &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Id").ToInt32() == 0 &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Version").ToInt32() == 2 &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Channel").ToInt32() == 3 &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Level").ToInt32() == 4 &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Opcode").ToInt32() == 5 &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Task").ToInt32() == 6 &&
                Marshal.OffsetOf(typeof(EventDescriptor), "Keyword").ToInt32() == 8 &&
                Marshal.OffsetOf(typeof(ProviderEventInfo), "NumberOfEvents").ToInt32() == 0 &&
                Marshal.OffsetOf(typeof(ProviderEventInfo), "Reserved").ToInt32() == 4 &&
                Marshal.OffsetOf(typeof(ProviderEventInfo), "FirstDescriptor").ToInt32() == HeaderBytes &&
                Marshal.SizeOf(typeof(ProviderEventInfo)) == HeaderBytes + DescriptorBytes;
        }

        // Pure decoder: tests call this without loading TDH or inspecting any provider.
        // returnedSize is the API's output size, not the original allocation size.
        public static Result Decode(byte[] buffer, uint returnedSize)
        {
            if (!SupportedLayout()) return Unavailable("tdh-layout-unsupported");
            if (buffer == null || buffer.Length > MaxBytes || returnedSize < HeaderBytes ||
                returnedSize > MaxBytes || returnedSize > buffer.Length)
                return Unavailable("tdh-buffer-invalid");
            uint count = BitConverter.ToUInt32(buffer, 0);
            if (BitConverter.ToUInt32(buffer, 4) != 0)
                return Unavailable("tdh-header-unsupported");
            if (count > MaxDescriptors) return Unavailable("tdh-descriptor-limit");
            // Exact-size validation also rejects incomplete descriptors and unknown tails.
            if ((ulong)HeaderBytes + (ulong)count * DescriptorBytes != returnedSize)
                return Unavailable("tdh-buffer-invalid");
            var descriptors = new Descriptor[(int)count];
            for (int i = 0; i < descriptors.Length; i++)
            {
                int offset = HeaderBytes + i * DescriptorBytes;
                descriptors[i] = new Descriptor {
                    id = BitConverter.ToUInt16(buffer, offset), version = buffer[offset + 2],
                    channel = buffer[offset + 3], level = buffer[offset + 4], opcode = buffer[offset + 5],
                    task = BitConverter.ToUInt16(buffer, offset + 6),
                    keyword_mask = "0x" + BitConverter.ToUInt64(buffer, offset + 8)
                        .ToString("X16", CultureInfo.InvariantCulture)
                };
            }
            return new Result { status = "ok", provider_guid = ProviderGuid,
                reason = null, api_status = null, descriptors = descriptors };
        }

        public static Result Collect()
        {
            if (Environment.OSVersion.Platform != PlatformID.Win32NT)
                return Unavailable("tdh-platform-unsupported");
            if (!SupportedLayout()) return Unavailable("tdh-layout-unsupported");
            IntPtr buffer = IntPtr.Zero;
            try
            {
                Guid provider = new Guid(ProviderGuid);
                uint required = 0;
                uint status = TdhEnumerateManifestProviderEvents(ref provider, IntPtr.Zero, ref required);
                if (status != 0 && status != 122) return Unavailable("tdh-probe-unavailable", status);
                if (required < HeaderBytes || required > MaxBytes)
                    return Unavailable("tdh-buffer-invalid");
                // Exactly one bounded allocation and fetch. Size changes never cause retries.
                buffer = Marshal.AllocHGlobal((int)required);
                // Reserved or unwritten bytes must never inherit allocator contents.
                Marshal.Copy(new byte[(int)required], 0, buffer, (int)required);
                uint returned = required;
                status = TdhEnumerateManifestProviderEvents(ref provider, buffer, ref returned);
                if (status != 0) return Unavailable("tdh-fetch-unavailable", status);
                if (returned < HeaderBytes || returned > required || returned > MaxBytes)
                    return Unavailable("tdh-buffer-invalid");
                byte[] bytes = new byte[(int)returned];
                Marshal.Copy(buffer, bytes, 0, bytes.Length);
                return Decode(bytes, returned);
            }
            catch (DllNotFoundException) { return Unavailable("tdh-api-unavailable"); }
            catch (EntryPointNotFoundException) { return Unavailable("tdh-api-unavailable"); }
            catch (BadImageFormatException) { return Unavailable("tdh-api-unavailable"); }
            catch (Exception) { return Unavailable("tdh-collection-unavailable"); }
            finally { if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer); }
        }
    }
}
