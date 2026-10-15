# Built from source like aur/PKGBUILD for Fedora and RHEL 10. CI rewrites
# Version from service/Cargo.toml.
# npm and cargo fetch dependencies over the network during %build, so build
# with plain rpmbuild (or mock --enable-network), not an offline buildroot.

# ring's C/asm objects fail to link under the default LTO flags
%global _lto_cflags %{nil}
# cargo's release profile emits no debuginfo, so there is nothing to split out
%global debug_package %{nil}

%global port 30912
# No %changelog entries to take a build date from (notes live in CHANGELOG.md)
%global source_date_epoch_from_changelog 0

Name:           framework-control
Version:        0.5.5
Release:        1%{?dist}
Summary:        Lightweight control surface for Framework laptops (Fan, Battery, Power, LEDs)

License:        MIT
URL:            https://github.com/ozturkkl/framework-control
Source0:        %{url}/archive/refs/tags/%{version}/%{name}-%{version}.tar.gz

ExclusiveArch:  x86_64

BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  nodejs
BuildRequires:  npm
BuildRequires:  systemd-rpm-macros

Requires:       hicolor-icon-theme
# Only the desktop entry needs it; RHEL servers and UBI images may not have it
Recommends:     xdg-utils
%{?systemd_requires}

%description
A local lightweight service with a Svelte web UI for Framework devices
(Laptop 12, 13, 16, and Desktop). It exposes a loopback HTTP API to monitor
telemetry and tweak fans, power, and charging.

framework_tool is downloaded automatically on first run if it is not
already on PATH.

%prep
%autosetup -n %{name}-%{version}

%build
cd web
npm ci
GITHUB_PAGES=false \
VITE_BASE="/" \
%{?announcement_url:VITE_ANNOUNCEMENT_URL="%{announcement_url}"} \
    npm run build

# Leave FRAMEWORK_CONTROL_UPDATE_REPO unset: the package manager owns the
# binary, so in-app self-updates stay disabled (same as the AUR package).
cd ../service
FRAMEWORK_CONTROL_PORT=%{port} \
FRAMEWORK_CONTROL_ALLOWED_ORIGINS="http://127.0.0.1:%{port}" \
    cargo build --release --locked --features embed-ui

%install
install -Dm755 service/target/release/framework-control-service %{buildroot}%{_bindir}/framework-control

# Icon generated during web build (guaranteed 512x512)
install -Dm644 web/public/assets/generated/icon-512.png \
    %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/framework-control.png

install -Dm644 /dev/stdin %{buildroot}%{_unitdir}/framework-control.service <<EOF
[Unit]
Description=Framework Control Service
After=network.target

[Service]
Type=simple
ExecStart=%{_bindir}/framework-control
Restart=on-failure
RestartSec=5
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
EOF

# Enable on install, matching the AUR package and install-linux.sh
install -Dm644 /dev/stdin %{buildroot}%{_presetdir}/80-framework-control.preset <<EOF
enable framework-control.service
EOF

install -Dm644 /dev/stdin %{buildroot}%{_datadir}/applications/framework-control.desktop <<EOF
[Desktop Entry]
Name=Framework Control
Comment=Configure Framework Laptop battery, fans, and LEDs
Exec=xdg-open http://127.0.0.1:%{port}
Icon=framework-control
Terminal=false
Type=Application
Categories=Settings;System;
EOF

%post
%systemd_post framework-control.service
# Start on first install; ignored where systemd isn't running (containers, rpm-ostree)
if [ $1 -eq 1 ]; then
    systemctl start framework-control.service >/dev/null 2>&1 || :
fi

%preun
%systemd_preun framework-control.service

%postun
%systemd_postun_with_restart framework-control.service

%files
%license LICENSE
%{_bindir}/framework-control
# The service downloads framework_tool next to its own binary on first run when
# it isn't on PATH; own those paths so dnf remove cleans them up
%ghost %attr(0755,root,root) %{_bindir}/framework_tool
%ghost %attr(0644,root,root) %{_bindir}/.framework_tool_latest_install_failed
%{_unitdir}/framework-control.service
%{_presetdir}/80-framework-control.preset
%{_datadir}/applications/framework-control.desktop
%{_datadir}/icons/hicolor/512x512/apps/framework-control.png

%changelog
# Release notes live in CHANGELOG.md
