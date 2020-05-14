import json
import re

prefix = "Az"
fn_prefix = "az_"
postfix = "Ptr"

azul_readme_path = "./azul/README.md"
license_path = "./LICENSE"
api_file_path = "./public.api.json"
rust_c_api_path = "./azul-dll/src/lib.rs"

bindings_c_path = "./azul/src/c/azul.h"
bindings_cpp_path = "./azul/src/cpp/azul.h"
bindings_rust_path = "./azul/src/rust/azul.rs"
bindings_python_path = "./azul/src/python/azul.py"

rust_c_api_header = "\
//! Public API for Azul\r\n\
//!\r\n\
//! A single function can have multiple implementations depending on whether it is\r\n\
//! compiled for the Rust-desktop target, the Rust-wasm target or the C API.\r\n\
//!\r\n\
//! For now, the crate simply re-exports azul_core and calls the c_api functions\r\n\
\r\n\
\r\n\
#![doc(\r\n\
    html_logo_url = \"https://raw.githubusercontent.com/maps4print/azul/master/assets/images/azul_logo_full_min.svg.png\",\r\n\
    html_favicon_url = \"https://raw.githubusercontent.com/maps4print/azul/master/assets/images/favicon.ico\",\r\n\
)]\r\n\
\r\n\
#![allow(dead_code)]\r\n\
extern crate azul_core;\r\n\
extern crate azul_css;\r\n\
extern crate azul_native_style;\r\n\
#[cfg(target_arch = \"wasm32\")]\r\n\
extern crate azul_web;\r\n\
#[cfg(not(target_arch = \"wasm32\"))]\r\n\
extern crate azul_desktop;\r\n\
\r\n\
use core::ffi::c_void;\r\n\
use azul_core::dom::Dom;\r\n\
use azul_core::callbacks::{RefAny, LayoutInfo};\r\n\
use azul_css::Css;\r\n\
use azul_core::window::WindowCreateOptions;\r\n\
#[cfg(not(target_arch = \"wasm32\"))]\r\n\
use azul_desktop::app::{App, AppConfig};\r\n\
#[cfg(target_arch = \"wasm32\")]\r\n\
use azul_web::app::{App, AppConfig};\r\n\
"

rust_typedefs = "\
/// The layout() callback fn\r\n\
pub type AzLayoutCallback = fn(AzRefAnyPtr, AzLayoutInfoPtr) -> AzDomPtr;\r\n\
"

c_typedefs = "\
// The layout() callback fn\r\n\
typedef AzDomPtr (*AzLayoutCallbackPtr)(AzRefAnyPtr, AzLayoutInfoPtr);\r\n\
"

rust_api_typedef = "\
    /// Callback fn that returns the layout\r\n\
    pub type LayoutCallback = fn(RefAny, LayoutInfo) -> Dom;\r\n\
    \r\n\
    \r\n\
    fn default_callback(_: RefAny, _: LayoutInfo) -> Dom {\r\n\
        Dom::div()\r\n\
    }\r\n\
    \r\n\
    pub(crate) static mut CALLBACK: LayoutCallback = default_callback;\r\n\
    \r\n\
    pub(crate) fn translate_callback(data: azul_dll::AzRefAnyPtr, layout: azul_dll::AzLayoutInfoPtr) -> azul_dll::AzDomPtr {\r\n\
        unsafe { CALLBACK(RefAny { ptr: data, run_destructor: true }, LayoutInfo { ptr: layout, run_destructor: true }) }.leak()\r\n\
    }\r\n\
"

rust_api_app_new_typedef = "{\r\n\
            unsafe { crate::callbacks::CALLBACK = callback };\r\n\
            az_app_new(config.leak(), data.leak(), crate::callbacks::translate_callback)\r\n\
        }\
"

def to_snake_case(name):
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1).lower()

def read_file(path):
    text_file = open(path, 'r')
    text_file_contents = text_file.read()
    temp = text_file_contents.decode('utf-8-sig')
    text_file_contents = temp.encode('utf-8')
    text_file.close()
    return text_file_contents

def read_api_file(path):
    api_file_contents = read_file(path)
    apiData = json.loads(api_file_contents,'utf-8')
    return apiData

def write_file(string, path):
    text_file = open(path, "w+")
    text_file.write(string)
    text_file.close()

def generate_c_api_code(apiData):

    code = "// WARNING: autogenerated code for azul api version " + str(apiData.keys()[-1]) + "\r\n"
    code += rust_c_api_header
    code += "\r\n\r\n"

    apiData = apiData.values()[-1]

    code += rust_typedefs
    code += "\r\n"

    for module_name in apiData.keys():
        module = apiData[module_name]
        for class_name in module.keys():
            c = module[class_name]

            code += "\r\n"

            rust_class_name = class_name
            if "rust_class_name" in c.keys():
                rust_class_name = c["rust_class_name"]

            class_ptr_name = prefix + class_name + postfix;

            if "doc" in c.keys():
                code += "/// " + c["doc"] + "\r\n"
            else:
                code += "/// Pointer to rust-allocated `Box<" + class_name + ">` struct\r\n"

            if "external" in c.keys():
                external_path = c["external"]
                code += "pub use ::" + external_path + " as " + class_ptr_name + ";\r\n"
            else:
                code += "#[no_mangle] #[repr(C)] pub struct " + class_ptr_name + " { ptr: *mut c_void }\r\n"

            if "constructors" in c.keys():
                for const in c["constructors"]:
                    if "doc" in const.keys():
                        code += "/// " + const["doc"] + "\r\n"
                    else:
                        code += "// Creates a new `" + class_name + "` instance whose memory is owned by the rust allocator\r\n"
                        code += "// Equivalent to the Rust `" + class_name  + "::" + const["fn_name"] + "()` constructor.\r\n"

                    fn_args = fn_args_c_api(const, class_name, class_ptr_name, False)

                    code += "#[no_mangle] pub extern \"C\" fn " + fn_prefix + to_snake_case(class_name) + "_" + const["fn_name"] + "(" + fn_args + ") -> " + class_ptr_name + " { "
                    code += class_ptr_name + " { ptr: Box::into_raw(Box::new(" + const["fn_body"] + ")) as *mut c_void }"
                    code += " }\r\n"

            if "functions" in c.keys():
                for f in c["functions"]:
                    if "doc" in f.keys():
                        code += "/// " + f["doc"] + "\r\n"
                    else:
                        code += "// Equivalent to the Rust `" + class_name  + "::" + f["fn_name"] + "()` function.\r\n"

                    fn_args = fn_args_c_api(f, class_name, class_ptr_name, True)

                    returns = ""
                    if "returns" in f.keys():
                        returns = " -> " + f["returns"]

                    code += "#[no_mangle] pub extern \"C\" fn " + fn_prefix + to_snake_case(class_name) + "_" + f["fn_name"] + "(" + fn_args + ")" + returns + " { "
                    code += f["fn_body"]
                    code += " }\r\n"

            lifetime = ""
            if "<'a>" in rust_class_name:
                lifetime = "<'a>"

            code += "/// Destructor: Takes ownership of the `" + class_name + "` pointer and deletes it.\r\n"
            code += "#[no_mangle] pub extern \"C\" fn " + fn_prefix + to_snake_case(class_name) + "_delete" + lifetime + "(ptr: &mut " + class_ptr_name + ") { "
            code += "let _ = unsafe { Box::<" + rust_class_name + ">::from_raw(ptr.ptr  as *mut " + rust_class_name + ") };"
            code += " }\r\n"

            code += "/// Copies the pointer: WARNING: After calling this function you'll have two pointers to the same Box<`" + class_name + "`>!.\r\n"
            code += "#[no_mangle] pub extern \"C\" fn " + fn_prefix + to_snake_case(class_name) + "_shallow_copy" + lifetime + "(ptr: &" + class_ptr_name + ") -> " + class_ptr_name + " { "
            code += class_ptr_name + " { ptr: ptr.ptr }"
            code += " }\r\n"

            code += "/// (private): Downcasts the `" + class_ptr_name + "` to a `Box<" + rust_class_name + ">`. Note that this takes ownership of the pointer.\r\n"
            code += "fn " + fn_prefix + to_snake_case(class_name) + "_downcast" + lifetime + "(ptr: " + class_ptr_name + ") -> Box<" + rust_class_name + "> { "
            code += "unsafe { Box::<" + rust_class_name + ">::from_raw(ptr.ptr  as *mut " + rust_class_name + ") }"
            code += " }\r\n"

    return code

def fn_args_c_api(f, class_name, class_ptr_name, self_as_first_arg):
    fn_args = ""

    if self_as_first_arg:
        self_val = f["args"]["self"]
        if (self_val == "value"):
            fn_args += class_name.lower() + ": " + class_ptr_name + ", "
        elif (self_val == "refmut"):
            fn_args += class_name.lower() + ": &mut" + class_ptr_name + ", "
        elif (self_val == "ref"):
            fn_args += class_name.lower() + ": &" + class_ptr_name + ", "
        else:
            raise Exception("wrong self value " + self_val)

    if "args" in f.keys():
        for arg_name in f["args"].keys():
            if arg_name == "self":
                continue
            arg_type = f["args"][arg_name]
            # special cases: no "Ptr" postfix
            if ((arg_type == "LayoutCallback") or (arg_type == "DataModel")):
                fn_args += arg_name + ": " + prefix + arg_type + ", " # no postfix
            else:
                fn_args += arg_name + ": " + prefix + arg_type + postfix + ", "
        fn_args = fn_args[:-2]

    return fn_args

# Generates the azul.h header
def generate_c_bindings(apiData):
    header = "// WARNING: autogenerated code for azul api version " + str(apiData.keys()[-1]) + "\r\n\r\n"
    apiData = apiData.values()[-1]

    license = read_file(license_path)

    for line in license.splitlines():
        header += "// " + line + "\r\n"
    header += "\r\n\r\n"

    header += "#ifndef AZUL_GUI_H\r\n"
    header += "#define AZUL_GUI_H\r\n"
    header += "\r\n"
    header += "#include <stdarg.h>\r\n"
    header += "#include <stdbool.h>\r\n"
    header += "#include <stdint.h>\r\n"
    header += "#include <stdlib.h>\r\n"
    header += "\r\n"
    header += "\r\n"

    header += c_typedefs

    header += "\r\n"
    header += "\r\n"

    for module_name in apiData.keys():
        module = apiData[module_name]
        for class_name in module.keys():
            c = module[class_name]
            header += "\r\n"

            class_ptr_name = prefix + class_name + postfix;

            if "doc" in c.keys():
                header += "// " + c["doc"] + "\r\n"
            else:
                header += "// Pointer to rust-allocated `Box<" + class_name + ">` struct\r\n"

            header += "typedef struct " + class_ptr_name + " { void *ptr; } "  +  class_ptr_name + "\r\n"

            if "constructors" in c.keys():
                for const in c["constructors"]:
                    if "doc" in const.keys():
                        header += "// " + const["doc"] + "\r\n"
                    else:
                        header += "// Creates a new `" + class_name + "` instance whose memory is owned by the rust allocator\r\n"
                        header += "// Equivalent to the Rust `" + class_name  + "::" + const["fn_name"] + "()` constructor.\r\n"

                    fn_args = get_fn_args_c(const, class_name, class_ptr_name)

                    fn_name = fn_prefix + to_snake_case(class_name) + "_" + const["fn_name"]
                    header += class_ptr_name + " " + fn_name + "(" + fn_args + ");\r\n"

            if "functions" in c.keys():
                for f in c["functions"]:
                    if "doc" in f.keys():
                        header += "// " + f["doc"] + "\r\n"
                    else:
                        header += "// Equivalent to the Rust `" + class_name  + "::" + f["fn_name"] + "()` function.\r\n"

                    fn_args = get_fn_args_c(f, class_name, class_ptr_name)

                    fn_name = fn_prefix + to_snake_case(class_name) + "_" + f["fn_name"]
                    header += class_ptr_name + " " + fn_name + "(" + fn_args + ");\r\n"

            header += "// Destructor: Takes ownership of the `" + class_name + "` pointer and deletes it.\r\n"
            header += "void " + fn_prefix + to_snake_case(class_name) + "_delete(" + class_ptr_name + "* ptr);\r\n"

    header += "\r\n\r\n#endif /* AZUL_GUI_H */\r\n"

    return header

def get_fn_args_c(f, class_name, class_ptr_name):
    fn_args = ""

    if "args" in f.keys():
        for arg_name in f["args"]:
            if arg_name == "self":
                continue
            arg_type = f["args"][arg_name]
            # special cases: no "Ptr" postfix
            if ((arg_type == "LayoutCallback") or (arg_type == "DataModel")):
                fn_args += prefix + arg_type + arg_name + " " + ", " # no postfix
            else:
                fn_args += prefix + arg_type + postfix + arg_name + " " + ", "
        fn_args = fn_args[:-2]
        if (len(f["args"]) == 0):
            fn_args = "void"

    return fn_args

# TODO
def generate_cpp_bindings(apiData):
    return generate_c_bindings(apiData)

def search_for_module_of_class(apiData, class_name):
    for module_name in apiData.keys():
        if class_name in apiData[module_name].keys():
            return module_name

    return None

def get_all_imports(apiData, module, module_name, existing_imports = {}):
    imports = existing_imports

    for class_name in module.keys():
        c = module[class_name]

        if "constructors" in c.keys():
            for const in c["constructors"]:
                if "args" in const.keys():
                    for arg_name in const["args"].keys():
                        if arg_name == "self":
                            continue
                        arg_type = const["args"][arg_name]
                        found_module = None

                        if arg_type == "LayoutCallback":
                            found_module = "callbacks"
                        else:
                            found_module = search_for_module_of_class(apiData, arg_type)

                        if found_module is None:
                            raise Exception("" + arg_type + " not found!")

                        if found_module in imports:
                            imports[found_module].append(arg_type)
                        else:
                            imports[found_module] = [arg_type]

        if "functions" in c.keys():
            for f in c["functions"]:
                if "args" in f.keys():
                    for arg_name in f["args"].keys():
                        if arg_name == "self":
                            continue
                        arg_type = f["args"][arg_name]
                        found_module = None

                        if arg_type == "LayoutCallback":
                            found_module = "callbacks"
                        else:
                            found_module = search_for_module_of_class(apiData, arg_type)

                        if found_module is None:
                            raise Exception("" + arg_type + " not found!")

                        if found_module in imports:
                            imports[found_module].append(arg_type)
                        else:
                            imports[found_module] = [arg_type]

    if module_name in imports:
        del imports[module_name]

    imports_str = ""

    for module_name in imports.keys():
        classes = imports[module_name]
        use_str = ""
        if len(classes) == 1:
            use_str = classes[0]
        else:
            use_str = "{"
            for c in classes:
                use_str += c + ", "
            use_str = use_str[:-2]
            use_str += "}"

        imports_str += "    use crate::" + module_name + "::" + use_str + ";\r\n"

    return imports_str

def generate_rust_bindings(apiData):

    code = "//! Auto-generated public Rust API for the Azul GUI toolkit version " + str(apiData.keys()[-1]) + "\r\n"
    code += "//!\r\n"

    readme = read_file(azul_readme_path)

    for line in readme.splitlines():
        code += "//! " + line + "\r\n"
    code += "\r\n"

    license = read_file(license_path)

    for line in license.splitlines():
        code += "// " + line + "\r\n"
    code += "\r\n\r\n"

    code += "extern crate azul_dll;"
    code += "\r\n\r\n"

    apiData = apiData.values()[-1]

    for module_name in apiData.keys():
        module = apiData[module_name]

        code += "pub mod " + module_name + " {\r\n\r\n"
        code += "    use azul_dll::*;\r\n"
        if module_name == "callbacks":
            code += get_all_imports(apiData, module, module_name, {"callbacks": ["RefAny", "LayoutInfo"], "dom": ["Dom"]})
            code += rust_api_typedef
        else:
            code += get_all_imports(apiData, module, module_name, {})

        for class_name in module.keys():
            c = module[class_name]

            class_ptr_name = prefix + class_name + postfix;

            code += "\r\n\r\n"

            if "doc" in c.keys():
                code += "    /// " + c["doc"] + "\r\n    "
            else:
                code += "    /// `" + class_name + "` struct\r\n    "

            code += "pub struct " + class_name + " { pub(crate) ptr: " +  class_ptr_name + ", pub(crate) run_destructor: bool }\r\n\r\n"

            code += "    impl " + class_name + " {\r\n"

            if "constructors" in c.keys():
                for const in c["constructors"]:
                    if "doc" in const.keys():
                        code += "        /// " + const["doc"] + "\r\n"
                    else:
                        code += "        /// Creates a new `" + class_name + "` instance.\r\n"

                    fn_args = rust_bindings_fn_args(const, class_name, class_ptr_name, False)
                    fn_args_call = rust_bindings_call_fn_args(const, class_name, class_ptr_name, False)
                    c_fn_name = fn_prefix + to_snake_case(class_name) + "_" + const["fn_name"]
                    fn_body = c_fn_name + "(" + fn_args_call + ")"

                    if [class_name, const["fn_name"]] == ["App", "new"]:
                        fn_body = rust_api_app_new_typedef

                    code += "        pub fn " + const["fn_name"] + "(" + fn_args + ") -> Self { Self { ptr: " + fn_body + ", run_destructor: true } }\r\n"

            if "functions" in c.keys():
                for f in c["functions"]:
                    if "doc" in f.keys():
                        code += "        /// " + f["doc"] + "\r\n"
                    else:
                        code += "        /// Calls the `" + class_name + "::" + f["fn_name"] + "` function.\r\n"

                    fn_args = rust_bindings_fn_args(f, class_name, class_ptr_name, True)
                    fn_args_call = rust_bindings_call_fn_args(f, class_name, class_ptr_name, True)
                    c_fn_name = fn_prefix + to_snake_case(class_name) + "_" + f["fn_name"]
                    fn_body = c_fn_name + "(" + fn_args_call + ")"

                    returns = ""
                    if "returns" in f.keys():
                        returns = " -> " + f["returns"]

                    code += "        pub fn " + f["fn_name"] + "(" + fn_args + ") " +  returns + " { " + fn_body + "}\r\n"

            code += "       /// Prevents the destructor from running and returns the internal `" + class_ptr_name + "`\r\n"
            code += "       #[allow(dead_code)]\r\n"
            code += "       pub(crate) fn leak(mut self) -> " + class_ptr_name + " { self.run_destructor = false; " +  fn_prefix + to_snake_case(class_name) + "_shallow_copy(&self.ptr) }\r\n"
            code += "    }\r\n\r\n"

            code += "    impl Drop for " + class_name + " { fn drop(&mut self) { if self.run_destructor { " + fn_prefix + to_snake_case(class_name) + "_delete(&mut self.ptr); } } }\r\n"

        code += "}\r\n\r\n"

    return code

# Generate the string for TAKING rust-api function arguments
def rust_bindings_fn_args(f, class_name, class_ptr_name, self_as_first_arg):
    fn_args = ""

    if self_as_first_arg:
        self_val = f["args"]["self"]
        if (self_val == "value"):
            fn_args += "self, "
        elif (self_val == "refmut"):
            fn_args += "&mut self, "
        elif (self_val == "ref"):
            fn_args += "&self, "
        else:
            raise Exception("wrong self value " + self_val)

    if "args" in f.keys():
        for arg_name in f["args"].keys():
            if arg_name == "self":
                continue
            arg_type = f["args"][arg_name]
            fn_args += arg_name + ": " + arg_type + ", "
        fn_args = fn_args[:-2]

    return fn_args

# Generate the string for CALLING rust-api function args
def rust_bindings_call_fn_args(f, class_name, class_ptr_name, self_as_first_arg):
    fn_args = ""
    if self_as_first_arg:
        self_val = f["args"]["self"]
        if (self_val == "value"):
            fn_args += "self.leak(), "
        elif (self_val == "refmut"):
            fn_args += "&mut self.ptr, "
        elif (self_val == "ref"):
            fn_args += "&self.ptr, "
        else:
            raise Exception("wrong self value " + self_val)

    if "args" in f.keys():
        for arg_name in f["args"]:
            arg_type = f["args"][arg_name]
            if arg_name == "self":
                continue

            if arg_type.startswith("&mut "):
                fn_args += "&mut " + arg_name + ".ptr, "
            elif arg_type.startswith("&"):
                fn_args += "&" + arg_name + ".ptr, "
            else:
                fn_args += arg_name + ".leak(), "

        fn_args = fn_args[:-2]

    return fn_args

def generate_python_bindings(apiData):
    return ""

def main():
    apiData = read_api_file(api_file_path)
    write_file(generate_c_api_code(apiData), rust_c_api_path)
    write_file(generate_c_bindings(apiData), bindings_c_path)
    write_file(generate_cpp_bindings(apiData), bindings_cpp_path)
    write_file(generate_rust_bindings(apiData), bindings_rust_path)
    write_file(generate_python_bindings(apiData), bindings_python_path)

if __name__ == "__main__":
    main()