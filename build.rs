//! Windows : intègre l'icône dans l'exécutable (explorateur, barre des
//! tâches, raccourcis). Sans effet ailleurs.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/boomground.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/boomground.ico");
        res.set("ProductName", "BoomGround");
        res.set("FileDescription", "BoomGround — destruction de voxels");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icône Windows non intégrée : {e}");
        }
    }
}
