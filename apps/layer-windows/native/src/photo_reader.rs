#[cfg(not(target_os = "windows"))]
pub(crate) use layer_host::photo_reader::local as read;
#[cfg(target_os = "windows")]
pub(crate) use confined::read;

#[cfg(target_os = "windows")]
mod confined {
    use layer_color::photo::{DecodeLimits, DecodedPhoto};
    use layer_host::photo_reader::{Input, active, arguments, helper};
    use std::{fs::File, os::windows::io::AsRawHandle, path::Path, sync::atomic::AtomicBool};
    use windows::{
        core::{HSTRING, PCWSTR, PWSTR, w},
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GENERIC_EXECUTE, GENERIC_READ, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, HLOCAL, LocalFree, SetHandleInformation, WAIT_TIMEOUT},
            Security::{ACL, DACL_SECURITY_INFORMATION, FreeSid, NO_INHERITANCE, PSECURITY_DESCRIPTOR, PSID, SECURITY_CAPABILITIES,
                Authorization::{EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W},
                Isolation::{CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName}},
            System::Threading::{CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE,
                InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject},
        },
    };

    pub(crate) fn read(input: &mut dyn Input, limits: DecodeLimits, cancelled: &AtomicBool) -> Result<DecodedPhoto, String> {
        let helper = helper()?;
        layer_host::photo_reader::decode(input, limits, cancelled, |job| run(&helper, &arguments(limits).join(" "), [&job.input, &job.output, &job.errors], cancelled))
    }

    struct Sid(PSID);
    impl Drop for Sid { fn drop(&mut self) { unsafe { FreeSid(self.0); } } }
    struct Owned(HANDLE);
    impl Drop for Owned { fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } } }
    struct Inherited<'a>(&'a [HANDLE; 3]);
    impl Drop for Inherited<'_> {
        fn drop(&mut self) { for handle in self.0 { unsafe { let _ = SetHandleInformation(*handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)); } } }
    }

    fn container() -> Result<Sid, String> {
        let name = w!("CapyCanvas.PhotoReader");
        match unsafe { CreateAppContainerProfile(name, name, name, None) } {
            Ok(sid) => Ok(Sid(sid)),
            Err(error) if error.code() == ERROR_ALREADY_EXISTS.to_hresult() => unsafe { DeriveAppContainerSidFromAppContainerName(name) }.map(Sid).map_err(|e| e.to_string()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn grant(file: &Path, sid: &Sid) {
        let path = HSTRING::from(file.as_os_str());
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            if GetNamedSecurityInfoW(&path, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, None, None, Some(&mut dacl), None, &mut descriptor).is_err() { return; }
            let access = EXPLICIT_ACCESS_W { grfAccessPermissions: GENERIC_READ.0 | GENERIC_EXECUTE.0, grfAccessMode: GRANT_ACCESS, grfInheritance: NO_INHERITANCE,
                Trustee: TRUSTEE_W { TrusteeForm: TRUSTEE_IS_SID, TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP, ptstrName: PWSTR(sid.0.0.cast()), ..Default::default() } };
            let mut updated: *mut ACL = std::ptr::null_mut();
            if SetEntriesInAclW(Some(&[access]), Some(dacl.cast_const()), &mut updated).is_ok() {
                let _ = SetNamedSecurityInfoW(&path, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, None, None, Some(updated.cast_const()), None);
                LocalFree(Some(HLOCAL(updated.cast())));
            }
            LocalFree(Some(HLOCAL(descriptor.0)));
        }
    }

    pub(super) fn run(program: &Path, arguments: &str, files: [&File; 3], cancelled: &AtomicBool) -> Result<bool, String> {
        let sid = container()?;
        let folder = program.parent().ok_or("Photo decoder is unavailable")?;
        for file in [program.to_path_buf(), folder.join("vcruntime140.dll"), folder.join("vcruntime140_1.dll")] {
            if file.is_file() { grant(&file, &sid); }
        }
        let handles = files.map(|file| HANDLE(file.as_raw_handle()));
        let _inherited = Inherited(&handles);
        for handle in &handles { unsafe { SetHandleInformation(*handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }.map_err(|e| e.to_string())?; }
        let mut size = 0usize;
        let _ = unsafe { InitializeProcThreadAttributeList(None, 2, None, &mut size) };
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe { InitializeProcThreadAttributeList(Some(list), 2, None, &mut size) }.map_err(|e| e.to_string())?;
        let capabilities = SECURITY_CAPABILITIES { AppContainerSid: sid.0, ..Default::default() };
        let mut command: Vec<u16> = format!("\"{}\" {arguments}", program.display()).encode_utf16().chain([0]).collect();
        let mut process = PROCESS_INFORMATION::default();
        let created = unsafe {
            UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize, Some((&raw const capabilities).cast()), size_of::<SECURITY_CAPABILITIES>(), None, None)
                .and_then(|()| UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, Some(handles.as_ptr().cast()), size_of_val(&handles), None, None))
                .and_then(|()| {
                    let startup = STARTUPINFOEXW { StartupInfo: STARTUPINFOW { cb: size_of::<STARTUPINFOEXW>() as u32, dwFlags: STARTF_USESTDHANDLES,
                        hStdInput: handles[0], hStdOutput: handles[1], hStdError: handles[2], ..Default::default() }, lpAttributeList: list };
                    CreateProcessW(&HSTRING::from(program.as_os_str()), Some(PWSTR(command.as_mut_ptr())), None, None, true,
                        EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW, None, PCWSTR::null(), &startup.StartupInfo, &mut process)
                })
        };
        unsafe { DeleteProcThreadAttributeList(list) };
        created.map_err(|e| format!("Photo decoder is unavailable: {e}"))?;
        let _thread = Owned(process.hThread);
        let child = Owned(process.hProcess);
        loop {
            if unsafe { WaitForSingleObject(child.0, 20) } != WAIT_TIMEOUT {
                let mut code = 0;
                unsafe { GetExitCodeProcess(child.0, &mut code) }.map_err(|e| e.to_string())?;
                return Ok(code == 0);
            }
            if let Err(error) = active(cancelled) {
                unsafe { let _ = TerminateProcess(child.0, 1); WaitForSingleObject(child.0, INFINITE); }
                return Err(error);
            }
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::{io::Write, path::Path, sync::atomic::AtomicBool};

    fn file() -> std::fs::File {
        layer_core::temp_files::anonymous(layer_core::temp_files::directory().unwrap(), "photo-test").unwrap()
    }

    #[test]
    fn photos_decode_in_an_app_container_that_cannot_read_user_files() {
        crate::test_support::temporary_files();
        let source = layer_core::color::source::rgba8_source([7, 5], |x, y| [x as u8 * 30, y as u8 * 40, 90, 255]);
        let png = layer_color::source_png(&source, Default::default(), usize::MAX, || false).unwrap();
        let limits = layer_color::photo::DecodeLimits { source_bytes: 1 << 20, codec_bytes: 1 << 20, dimension: 64 };
        let decoded = super::read(&mut std::io::Cursor::new(&png), limits, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.source.extent, [7, 5]);
        let cmd = Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32").join("cmd.exe");
        let launch = |arguments: &str| super::confined::run(&cmd, arguments, [&file(), &file(), &file()], &AtomicBool::new(false)).unwrap();
        assert!(launch("/c exit 0"), "the container starts");
        let private = std::env::temp_dir().join(format!("capy-photo-private-{}", std::process::id()));
        std::fs::File::create(&private).unwrap().write_all(b"private").unwrap();
        let readable = launch(&format!("/c type \"{}\"", private.display()));
        std::fs::remove_file(&private).unwrap();
        assert!(!readable, "the container cannot read the user's files");
    }
}
