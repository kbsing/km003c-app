// csharp_example.cs — P/Invoke into km003c_parse.dll.
//
// Build: csc /out:csharp_example.exe csharp_example.cs
// Run with km003c_parse.dll next to the exe (or on PATH).
using System;
using System.Runtime.InteropServices;

class Program
{
    [DllImport("km003c_parse.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr km003c_parse_pd_hex(string hex);

    [DllImport("km003c_parse.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern void km003c_free(IntPtr ptr);

    static void Main()
    {
        // Source_Capabilities with one PPS APDO.
        IntPtr ptr = km003c_parse_pd_hex("01116421dcc0");
        try
        {
            string json = Marshal.PtrToStringAnsi(ptr);
            Console.WriteLine(json);
        }
        finally
        {
            km003c_free(ptr);
        }
    }
}
