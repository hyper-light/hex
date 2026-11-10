
    #[test]
    fn render_legacy_capture() {
        let output = PathBuf::from(std::env::var_os("HEX_RENDER_CAPTURE").unwrap());
        let masked = ["/sys/host-private-mask".to_owned()];
        let mut rootless_dir = in_place();
        rootless_dir.init.user = None;
        let cases = [
            ("layers-root", layered(), at(Owner::Root, &[])),
            ("layers-rootless", layered(), at(ROOTLESS, &[])),
            ("dir-root", in_place(), at(Owner::Root, &masked)),
            ("dir-rootless", rootless_dir, at(ROOTLESS, &[])),
        ];
        for (name, spec, place) in cases {
            let rendered = render(&spec, &place).unwrap();
            std::fs::write(output.join(format!("{name}.config")), rendered.config).unwrap();
            std::fs::write(output.join(format!("{name}.hosts")), rendered.hosts).unwrap();
            std::fs::write(
                output.join(format!("{name}.dirs")),
                serde_json::to_vec(&rendered.dirs).unwrap(),
            )
            .unwrap();
        }
    }
