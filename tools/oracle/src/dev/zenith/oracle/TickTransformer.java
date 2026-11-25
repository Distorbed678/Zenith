// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassHierarchyResolver;
import java.lang.classfile.ClassTransform;
import java.lang.classfile.CodeBuilder;
import java.lang.classfile.CodeElement;
import java.lang.classfile.CodeTransform;
import java.lang.classfile.instruction.ReturnInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.ConstantDescs;
import java.lang.constant.MethodTypeDesc;
import java.lang.instrument.ClassFileTransformer;
import java.security.ProtectionDomain;

/**
 * Injects {@code Hooks.tickStart(this)} at the start of the configured tick method and {@code Hooks.tickEnd(this)}
 * before each of its returns. Nothing else in the method changes.
 */
final class TickTransformer implements ClassFileTransformer {
    private static final ClassDesc HOOKS = ClassDesc.of("dev.zenith.oracle.Hooks");
    private static final MethodTypeDesc OBJ_TO_VOID = MethodTypeDesc.of(ConstantDescs.CD_void, ConstantDescs.CD_Object);

    private final String internalName;
    private final String method;
    private final String descriptor;

    TickTransformer(String internalName, String method, String descriptor) {
        this.internalName = internalName;
        this.method = method;
        this.descriptor = descriptor;
    }

    @Override
    public byte[] transform(ClassLoader loader, String className, Class<?> redefined, ProtectionDomain domain, byte[] bytes) {
        if (!internalName.equals(className)) {
            return null;
        }
        try {
            ClassHierarchyResolver resolver = ClassHierarchyResolver.defaultResolver()
                    .orElse(ClassHierarchyResolver.ofResourceParsing(loader));
            ClassFile cf = ClassFile.of(ClassFile.ClassHierarchyResolverOption.of(resolver));
            byte[] out = cf.transformClass(cf.parse(bytes), ClassTransform.transformingMethodBodies(
                    m -> m.methodName().equalsString(method) && m.methodType().equalsString(descriptor),
                    new CodeTransform() {
                        @Override
                        public void atStart(CodeBuilder b) {
                            b.aload(0);
                            b.invokestatic(HOOKS, "tickStart", OBJ_TO_VOID);
                        }

                        @Override
                        public void accept(CodeBuilder b, CodeElement e) {
                            if (e instanceof ReturnInstruction) {
                                b.aload(0);
                                b.invokestatic(HOOKS, "tickEnd", OBJ_TO_VOID);
                            }
                            b.with(e);
                        }
                    }));
            System.out.println("[zenith-oracle] instrumented " + className.replace('/', '.') + "." + method);
            return out;
        } catch (Throwable t) {
            System.err.println("[zenith-oracle] failed to instrument " + className + ": " + t);
            t.printStackTrace();
            return null;
        }
    }
}
