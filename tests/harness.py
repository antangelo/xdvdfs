import filecmp
import os
import subprocess
import tempfile

CMD_BUILD_IMAGE = 'cargo r --bin xdvdfs -- build-image {options} {input} {output}'
CMD_IMAGE_SPEC = 'cargo r --bin xdvdfs -- image-spec from {options} {output}'

class TestCase:
    def __init__(self, name):
        self.name = name

    def run(self, dir, args):
        raise Exception('Test run not implemented')

    def set_up(self, dir):
        raise Exception('Test case not implemented')

def run_cmd_with_io(cmd, input, output, options = None):
    assert cmd is not None
    pack_cmd = cmd.format(input = input, output = output, options = options).split(' ')
    proc = subprocess.run(pack_cmd, capture_output=True)
    if proc.returncode != 0:
        raise Exception('Failed to run command')

def test_pack_unpack(dir, output_img, args):
    output_dir = tempfile.TemporaryDirectory()
    run_cmd_with_io(args.unpack_cmd, output_img.name, output_dir.name)

    l1_cmp = filecmp.dircmp(dir, output_dir.name)
    if l1_cmp.diff_files or l1_cmp.left_only or l1_cmp.right_only or l1_cmp.funny_files:
        l1_cmp.report_full_closure()
        raise Exception('Mismatch between test case input and packed output')

def test_repack_unpack(dir, output_img, args):
    repack_img = tempfile.NamedTemporaryFile()
    repack_output = tempfile.TemporaryDirectory()
    run_cmd_with_io(args.repack_cmd, output_img.name, repack_img.name)
    run_cmd_with_io(args.unpack_cmd, repack_img.name, repack_output.name)

    l2_cmp = filecmp.dircmp(dir, repack_output.name)
    if l2_cmp.diff_files or l2_cmp.left_only or l2_cmp.right_only or l2_cmp.funny_files:
        l2_cmp.report_full_closure()
        raise Exception('Mismatch between test case input and repacked output')

class PackTestCase(TestCase):
    def __init__(self, name):
        super().__init__(name)

    def run(self, dir, args):
        img_file = tempfile.NamedTemporaryFile()
        run_cmd_with_io(args.pack_cmd, dir, img_file.name)

        test_pack_unpack(dir, img_file, args)
        test_repack_unpack(dir, img_file, args)

class BuildImageTestCase(TestCase):
    def __init__(self, name, build_image_opts):
        super().__init__(name)
        self._build_image_opts = build_image_opts

    def set_up(self, dir):
        os.mkdir(dir + '/source')
        os.mkdir(dir + '/dest')

    def run(self, dir, args):
        # Pass through the build_image_opts first
        img_file = tempfile.NamedTemporaryFile()
        run_cmd_with_io(CMD_BUILD_IMAGE, dir + '/source', img_file.name, self._build_image_opts)
        test_pack_unpack(dir + '/dest', img_file, args)

        # Create an image_spec.toml file and build the image from config
        spec_file = tempfile.NamedTemporaryFile()
        img_file2 = tempfile.NamedTemporaryFile()
        run_cmd_with_io(CMD_IMAGE_SPEC, None, spec_file.name, self._build_image_opts)
        run_cmd_with_io(CMD_BUILD_IMAGE, dir + '/source', img_file2.name, '-f {}'.format(spec_file.name))
        test_pack_unpack(dir + '/dest', img_file2, args)
