if RUST_DEBUG
    CARGO_RELEASE_ARGS=
else
    CARGO_RELEASE_ARGS=--release
endif

CARGO_VERBOSE = $(cargo_verbose_$(V))
cargo_verbose_ = $(cargo_verbose_$(AM_DEFAULT_VERBOSITY))
cargo_verbose_0 =
cargo_verbose_1 = --verbose

EXTRA_DIST = Cargo.lock
bin_PROGRAMS = xnde

xnde_SOURCES = Cargo.toml src/vars.rs src/main.rs src/lib.rs src/tracks.rs src/fields.rs

# TODO(sp1ff): for some reason, the --version flag isn't working ATM. Since I'm building against
# the source, I'm not going to debug it at the moment.
AM_INSTALLCHECK_STD_OPTIONS_EXEMPT = xnde

xnde$(EXEEXT): $(xnde_SOURCES)
	cd $(top_srcdir)/xnde && \
	CARGO_TARGET_DIR=@abs_top_builddir@/xnde/target cargo build $(CARGO_VERBOSE) $(CARGO_RELEASE_ARGS)

clean-local:
	cd $(top_srcdir)/xnde && \
	CARGO_TARGET_DIR=@abs_top_builddir@/xnde/target cargo clean

check-local:
	cd $(top_srcdir)/xnde && \
	CARGO_TARGET_DIR=@abs_top_builddir@/xnde/target cargo test

install-exec-local: xnde$(EXEEXT)
	install -d $(DESTDIR)$(bindir)
	install -m 755 @abs_top_builddir@/xnde/target/$(RUST_TARGET_SUBDIR)/xnde $(DESTDIR)$(bindir)/xnde
